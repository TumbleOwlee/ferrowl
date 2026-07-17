//! Version-generic OCPP charging-station backend: wraps the `ferrowl-ocpp` `Client<V>` for any
//! OCPP version, tracks websocket online state, and records every request/response payload for the
//! view. Sending is fully generic — the caller builds a typed `V::Action` (e.g. from a UI action
//! button) and the backend records the request JSON, awaits the reply, and records the response.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::sync::RwLock;
use tokio::sync::mpsc;

use ferrowl_ocpp::cs::{Client, ClientBuilder, Command, Config, CsActionHandler};
use ferrowl_ocpp::{Error, Version};

use crate::module::ocpp::config::session::OcppSpec;
use crate::module::ocpp::scope::Scope;
use crate::module::ocpp::wire_log::{encode_action_or_log, encode_response_or_log};

/// Number of `refresh` ticks per second (the UI ticks at ~100ms), used to convert second-based
/// cadences (heartbeat interval, MeterValues period) into tick counts.
pub const TICKS_PER_SEC: u32 = 10;

/// Fallback heartbeat cadence (seconds) used until the CSMS supplies one in its BootNotification
/// response (or when it sends `0`).
pub const DEFAULT_HEARTBEAT_SECS: u64 = 30;

/// Extract the heartbeat cadence from a BootNotification response. The CSMS dictates the interval;
/// an absent or zero value yields `None`, so the caller falls back to [`DEFAULT_HEARTBEAT_SECS`].
pub fn boot_interval(response: &Value) -> Option<u64> {
    response["interval"].as_u64().filter(|&i| i > 0)
}

/// Direction of a recorded OCPP message relative to this charging station.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Received from the CSMS (an inbound Call, or a reply to our Call).
    In,
    /// Sent to the CSMS (our Call, or our reply to an inbound Call).
    Out,
}

impl Dir {
    pub fn label(self) -> &'static str {
        match self {
            Dir::In => "Inbound",
            Dir::Out => "Outbound",
        }
    }
}

/// Largest number of messages kept in an in-memory message-log buffer. Older messages are evicted;
/// the full history survives only in the `:log <file>` sink (via the per-module `SharedLog`).
pub const MAX_MESSAGES: usize = 200;

/// Monotonic message sequence, so a view can tee only the messages it hasn't logged yet even as the
/// bounded buffer evicts older ones. Starts at 1 so a cursor initialised to 0 logs the first message.
static MSG_SEQ: AtomicU64 = AtomicU64::new(1);

fn next_seq() -> u64 {
    MSG_SEQ.fetch_add(1, Ordering::Relaxed)
}

/// One observed OCPP message, for the message-log table and the JSON view.
#[derive(Debug, Clone)]
pub struct OcppMessage {
    /// Monotonic id assigned at creation (see [`MSG_SEQ`]); used by the log tee cursor.
    pub seq: u64,
    pub ts: u64,
    pub direction: Dir,
    pub name: String,
    pub payload: Value,
    /// Outcome: `Some(true)` = success, `Some(false)` = error, `None` = neutral (e.g. a request).
    pub ok: Option<bool>,
    /// Extra context: a status string on success, an error message on failure.
    pub context: String,
    /// The connector/CS scope this message belongs to, for the client view's per-connector message
    /// filtering. CS-level (Boot/Heartbeat/etc.) is [`Scope::CS`]; connector traffic carries its
    /// connector scope. The server view ignores this (it routes by its own per-entry log).
    pub scope: Scope,
}

impl OcppMessage {
    /// Build a CS-level message, stamping it with the current time and the next sequence id.
    pub fn new(
        direction: Dir,
        name: impl Into<String>,
        payload: Value,
        ok: Option<bool>,
        context: impl Into<String>,
    ) -> Self {
        Self::new_scoped(Scope::CS, direction, name, payload, ok, context)
    }

    /// Build a message tagged with a connector/CS [`Scope`].
    pub fn new_scoped(
        scope: Scope,
        direction: Dir,
        name: impl Into<String>,
        payload: Value,
        ok: Option<bool>,
        context: impl Into<String>,
    ) -> Self {
        Self {
            seq: next_seq(),
            ts: now_ms(),
            direction,
            name: name.into(),
            payload,
            ok,
            context: context.into(),
            scope,
        }
    }

    /// A one-line rendering for the persistent log: direction, name, outcome, context, payload.
    pub fn log_line(&self) -> String {
        let dir = match self.direction {
            Dir::In => "<-",
            Dir::Out => "->",
        };
        let status = match self.ok {
            Some(true) => " ok",
            Some(false) => " ERR",
            None => "",
        };
        let ctx = if self.context.is_empty() {
            String::new()
        } else {
            format!(" ({})", self.context)
        };
        let payload = serde_json::to_string(&self.payload).unwrap_or_default();
        format!("{dir} {}{status}{ctx} {payload}", self.name)
    }
}

pub type Messages = Arc<RwLock<Vec<OcppMessage>>>;

/// Push a message into a buffer, evicting the oldest once it exceeds [`MAX_MESSAGES`].
pub fn push_capped(buf: &mut Vec<OcppMessage>, msg: OcppMessage) {
    buf.push(msg);
    if buf.len() > MAX_MESSAGES {
        let overflow = buf.len() - MAX_MESSAGES;
        buf.drain(0..overflow);
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// --- Message table -----------------------------------------------------------
//
// Shared between the client and server views' message-log tables (identical row shape, styling,
// and construction from an `OcppMessage` in both), kept here next to `OcppMessage` itself.

#[derive(Clone, Debug, ferrowl_ui_derive::TableEntry)]
#[table_entry(header = MsgHeader, styles = msg_cell_styles)]
pub(crate) struct MsgRow {
    #[column(name = "Timestamp", min = 23, max = 23)]
    timestamp: String,
    #[column(name = "Direction", min = 8, max = 10)]
    direction: String,
    #[column(name = "Message", min = 14, max = 30)]
    name: String,
    #[column(name = "Status", min = 7, max = 8)]
    status: String,
    #[column(name = "Context", min = 6, max = 40)]
    context: String,
}

pub(crate) fn msg_cell_styles(row: &MsgRow) -> [Option<ratatui::style::Style>; 5] {
    let status_style = match row.status.as_str() {
        "Success" => Some(ratatui::style::Style::default().fg(ferrowl_ui::COLOR_SCHEME.success)),
        "Error" => Some(ratatui::style::Style::default().fg(ferrowl_ui::COLOR_SCHEME.error)),
        _ => None,
    };
    [None, None, None, status_style, None]
}

pub(crate) fn msg_row(m: &OcppMessage) -> MsgRow {
    let status = match m.ok {
        Some(true) => "Success",
        Some(false) => "Error",
        None => "",
    };
    MsgRow {
        timestamp: crate::view::log::format_timestamp(m.ts),
        direction: m.direction.label().to_string(),
        name: m.name.clone(),
        status: status.to_string(),
        context: m.context.clone(),
    }
}

/// The version-generic charging-station backend owned by a client view.
/// Deliberately holds no copy of the module spec: the connection config is built from the spec
/// the view passes into each [`start`](Self::start) call (see the CSMS backend for the rationale).
pub struct OcppClient<V: Version> {
    client: Option<Client<V>>,
    online: Arc<AtomicBool>,
    messages: Messages,
}

impl<V: Version> OcppClient<V> {
    pub fn new() -> Self {
        Self {
            client: None,
            online: Arc::new(AtomicBool::new(false)),
            messages: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Shared online flag, for an inbound handler to flip on connect/disconnect.
    pub fn online_handle(&self) -> Arc<AtomicBool> {
        self.online.clone()
    }

    /// Shared message buffer, for an inbound handler to record into.
    pub fn messages_handle(&self) -> Messages {
        self.messages.clone()
    }

    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }

    pub async fn messages_snapshot(&self) -> Vec<OcppMessage> {
        self.messages.read().await.clone()
    }

    /// A detachable sender for off-thread Calls (or `None` cmd channel when not connected). The
    /// returned value owns clones of the command channel and message log, so the caller can
    /// `tokio::spawn` the round-trip and keep the UI responsive while the peer is slow/silent.
    pub fn sender(&self) -> OcppSender<V> {
        OcppSender {
            cmd_tx: self.client.as_ref().map(|c| c.sender()),
            messages: self.messages.clone(),
        }
    }

    /// Dial the CSMS and spawn the client task, using the caller-supplied inbound handler.
    /// The connection config is built from the caller's `spec` on every call — the backend holds
    /// no copy, so an edited endpoint/security section always takes effect on the next start.
    pub async fn start<H: CsActionHandler<V>>(
        &mut self,
        spec: &OcppSpec,
        handler: H,
    ) -> Result<(), Error> {
        if self.client.is_some() {
            // Already connected: nothing to do. But if the websocket dropped without an explicit
            // `stop` the handle is stale (task dead, `online` already false) — tear it down so we
            // can redial instead of silently no-op'ing.
            if self.is_online() {
                return Ok(());
            }
            let _ = self.stop().await;
        }
        let config = Config {
            url: spec.url(),
            timeout_ms: spec.timeout_ms.unwrap_or(30_000),
            basic_auth: spec.security.basic_auth(),
            tls: spec.security.cs_tls(),
        };
        let log = log_fn(self.messages.clone());
        let client = ClientBuilder::<V>::new(config).spawn(handler, log).await?;
        self.client = Some(client);
        // The handshake already completed in `spawn`; reflect it immediately (the handler's
        // on_connected callback keeps it in sync thereafter).
        self.online.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// Terminate the client task, if running.
    pub async fn stop(&mut self) -> Result<(), Error> {
        self.online.store(false, Ordering::Relaxed);
        match self.client.take() {
            Some(c) => c.terminate().await,
            None => Ok(()),
        }
    }
}

/// A self-contained Call sender, decoupled from the [`OcppClient`] borrow so the round-trip can be
/// `tokio::spawn`ed. Records the request and reply into the same shared message log the view reads.
pub struct OcppSender<V: Version> {
    cmd_tx: Option<mpsc::Sender<Command<V>>>,
    messages: Messages,
}

impl<V: Version> OcppSender<V> {
    /// Send a typed Call tagging the recorded request/reply with `scope`, so the client view can
    /// filter the message log per connector. Returns the response JSON on success. Awaiting this
    /// never blocks the UI loop because the caller spawns it.
    pub async fn send_scoped(self, action: V::Action, scope: Scope) -> Result<Value, Error> {
        let name = V::action_name(&action).to_string();
        let request = encode_action_or_log::<V>(&action);
        record(
            &self.messages,
            scope,
            Dir::Out,
            &name,
            request,
            None,
            String::new(),
        )
        .await;

        let result = match &self.cmd_tx {
            Some(cmd_tx) => Client::<V>::call_via(cmd_tx, action).await,
            None => Err(Error::NotRunning),
        };
        match result {
            Ok(response) => {
                let payload = encode_response_or_log::<V>(&response);
                record(
                    &self.messages,
                    scope,
                    Dir::In,
                    &name,
                    payload.clone(),
                    Some(true),
                    String::new(),
                )
                .await;
                Ok(payload)
            }
            Err(e) => {
                let msg = e.to_string();
                record(
                    &self.messages,
                    scope,
                    Dir::In,
                    &name,
                    Value::Null,
                    Some(false),
                    msg,
                )
                .await;
                Err(e)
            }
        }
    }
}

/// Push one message into a shared message log (bounded to [`MAX_MESSAGES`]).
async fn record(
    messages: &Messages,
    scope: Scope,
    dir: Dir,
    name: &str,
    payload: Value,
    ok: Option<bool>,
    context: String,
) {
    let mut guard = messages.write().await;
    push_capped(
        &mut guard,
        OcppMessage::new_scoped(scope, dir, name, payload, ok, context),
    );
}

/// A `LogFn` that records error/diagnostic strings into the message log.
fn log_fn(
    messages: Messages,
) -> impl Fn(String) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Clone {
    move |s: String| {
        let messages = messages.clone();
        Box::pin(async move {
            let mut guard = messages.write().await;
            push_capped(
                &mut guard,
                OcppMessage::new(Dir::In, "log", Value::String(s), None, String::new()),
            );
        })
    }
}

/// Format a Unix-millisecond timestamp as an RFC3339 UTC string (`YYYY-MM-DDTHH:MM:SSZ`).
pub fn rfc3339(ms: u64) -> String {
    let total_secs = ms / 1000;
    let h = (total_secs / 3600) % 24;
    let m = (total_secs / 60) % 60;
    let s = total_secs % 60;
    let (year, month, day) = civil_from_days((total_secs / 86400) as i64);
    format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
}

/// RFC3339 UTC string for the current time.
pub fn rfc3339_now() -> String {
    rfc3339(now_ms())
}

/// Days since the Unix epoch to a Gregorian (year, month, day) triple (Howard Hinnant).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era: i64 = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    /// OC-R-060 — the CS heartbeat cadence uses the interval the CSMS returned in its BootNotification response.
    fn boot_interval_prefers_csms_value() {
        let resp =
            json!({ "currentTime": "2026-01-01T00:00:00Z", "interval": 120, "status": "Accepted" });
        assert_eq!(boot_interval(&resp), Some(120));
    }

    #[test]
    /// OC-R-060 — the heartbeat cadence falls back to 30 s when the CSMS interval is absent or zero.
    fn boot_interval_falls_back_on_zero_or_absent() {
        // Zero is treated as "unset" so the caller uses DEFAULT_HEARTBEAT_SECS.
        assert_eq!(boot_interval(&json!({ "interval": 0 })), None);
        // Missing field.
        assert_eq!(boot_interval(&json!({ "status": "Accepted" })), None);
    }

    #[test]
    /// OC-R-083 — a client module does not connect automatically; a freshly built backend is offline until an explicit start.
    fn new_client_backend_is_offline_until_started() {
        let backend = OcppClient::<ferrowl_ocpp::V1_6>::new();
        assert!(!backend.is_online());
    }

    #[test]
    /// OC-R-088 — message sequence numbers are strictly increasing and unique, the cursor a file-log tee uses to write each message at most once.
    fn message_seqs_are_strictly_increasing() {
        let a = next_seq();
        let b = next_seq();
        let c = next_seq();
        assert!(a < b && b < c);
    }

    #[test]
    /// OC-R-087 — the in-memory message log is bounded to the most recent 200 messages, evicting oldest-first.
    fn push_capped_evicts_oldest_beyond_limit() {
        let mut buf = Vec::new();
        for _ in 0..(MAX_MESSAGES + 50) {
            push_capped(
                &mut buf,
                OcppMessage::new(Dir::Out, "Heartbeat", json!({}), None, String::new()),
            );
        }
        assert_eq!(buf.len(), MAX_MESSAGES);
        // The retained window is the newest MAX_MESSAGES, so seqs are strictly increasing and the
        // front is no longer seq 1.
        assert!(buf[0].seq < buf[buf.len() - 1].seq);
        assert!(buf.windows(2).all(|w| w[0].seq < w[1].seq));
    }

    #[test]
    /// OC-R-078 — each recorded message renders its direction, status, and payload for display and logging.
    fn log_line_renders_direction_status_and_payload() {
        let m = OcppMessage::new(
            Dir::In,
            "BootNotification",
            json!({"status":"Accepted"}),
            Some(true),
            "boot",
        );
        let line = m.log_line();
        assert!(line.starts_with("<- BootNotification ok (boot) "));
        assert!(line.contains("\"status\":\"Accepted\""));
    }

    #[test]
    fn ut_rfc3339_formats_epoch_and_known_instant() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        // 1_000_000_000 s since the epoch is 2001-09-09T01:46:40Z.
        assert_eq!(rfc3339(1_000_000_000_000), "2001-09-09T01:46:40Z");
    }

    #[test]
    fn ut_dir_label() {
        assert_eq!(Dir::In.label(), "Inbound");
        assert_eq!(Dir::Out.label(), "Outbound");
    }

    #[test]
    fn ut_new_scoped_tags_message_with_scope() {
        let m = OcppMessage::new_scoped(
            Scope::connector(3),
            Dir::Out,
            "StatusNotification",
            json!({}),
            None,
            "",
        );
        assert_eq!(m.scope, Scope::connector(3));
        assert_eq!(m.direction, Dir::Out);
    }

    #[test]
    /// OC-R-078 — a recorded message renders into the log table with its direction and status.
    fn ut_msg_row_and_cell_styles() {
        let ok = OcppMessage::new(Dir::In, "Boot", json!({}), Some(true), "ctx");
        let row = msg_row(&ok);
        assert_eq!(row.direction, "Inbound");
        assert_eq!(row.status, "Success");
        assert_eq!(row.context, "ctx");
        // The status cell (index 3) is styled for success/error, others are unstyled.
        let styles = msg_cell_styles(&row);
        assert!(styles[3].is_some());
        assert!(styles[0].is_none());
        let err = msg_row(&OcppMessage::new(
            Dir::Out,
            "Boot",
            json!({}),
            Some(false),
            "",
        ));
        assert_eq!(err.status, "Error");
        assert!(msg_cell_styles(&err)[3].is_some());
        let neutral = msg_row(&OcppMessage::new(Dir::Out, "Boot", json!({}), None, ""));
        assert_eq!(neutral.status, "");
        assert!(msg_cell_styles(&neutral)[3].is_none());
    }
}
