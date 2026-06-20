//! Version-generic OCPP charging-station backend: wraps the `ferrowl-ocpp` `Client<V>` for any
//! OCPP version, tracks websocket online state, and records every request/response payload for the
//! view. Sending is fully generic — the caller builds a typed `V::Action` (e.g. from a UI action
//! button) and the backend records the request JSON, awaits the reply, and records the response.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tokio::sync::RwLock;

use ferrowl_ocpp::cs::{Client, ClientBuilder, Config, CsActionHandler};
use ferrowl_ocpp::{Error, Version};

use crate::module::ocpp::config::session::OcppSpec;

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

/// One observed OCPP message, for the message-log table and the JSON view.
#[derive(Debug, Clone)]
pub struct OcppMessage {
    pub ts: u64,
    pub direction: Dir,
    pub name: String,
    pub payload: Value,
    /// Outcome: `Some(true)` = success, `Some(false)` = error, `None` = neutral (e.g. a request).
    pub ok: Option<bool>,
    /// Extra context: a status string on success, an error message on failure.
    pub context: String,
}

pub type Messages = Arc<RwLock<Vec<OcppMessage>>>;

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// The version-generic charging-station backend owned by a client view.
pub struct OcppClient<V: Version> {
    spec: OcppSpec,
    client: Option<Client<V>>,
    online: Arc<AtomicBool>,
    messages: Messages,
}

impl<V: Version> OcppClient<V> {
    pub fn new(spec: OcppSpec) -> Self {
        Self {
            spec,
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

    /// Dial the CSMS and spawn the client task, using the caller-supplied inbound handler.
    pub async fn start<H: CsActionHandler<V>>(&mut self, handler: H) -> Result<(), Error> {
        if self.client.is_some() {
            return Ok(());
        }
        let config = Config {
            url: self.spec.url(),
            timeout_ms: self.spec.timeout_ms.unwrap_or(30_000),
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

    /// Send a typed Call, recording the request payload and the response (or error). Returns the
    /// response payload JSON on success so callers can read fields (e.g. a transaction id).
    pub async fn send(&self, action: V::Action) -> Result<Value, Error> {
        let name = V::action_name(&action).to_string();
        let request = V::encode_action(&action).unwrap_or(Value::Null);
        self.record(Dir::Out, &name, request, None, String::new())
            .await;

        let result = match &self.client {
            Some(c) => c.call(action).await,
            None => Err(Error::NotRunning),
        };
        match result {
            Ok(response) => {
                let payload = V::encode_response(&response).unwrap_or(Value::Null);
                self.record(Dir::In, &name, payload.clone(), Some(true), String::new())
                    .await;
                Ok(payload)
            }
            Err(e) => {
                let msg = e.to_string();
                self.record(Dir::In, &name, Value::Null, Some(false), msg).await;
                Err(e)
            }
        }
    }

    async fn record(&self, dir: Dir, name: &str, payload: Value, ok: Option<bool>, context: String) {
        self.messages.write().await.push(OcppMessage {
            ts: now_ms(),
            direction: dir,
            name: name.to_string(),
            payload,
            ok,
            context,
        });
    }
}

/// A `LogFn` that records error/diagnostic strings into the message log.
fn log_fn(
    messages: Messages,
) -> impl Fn(String) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send>> + Clone {
    move |s: String| {
        let messages = messages.clone();
        Box::pin(async move {
            messages.write().await.push(OcppMessage {
                ts: now_ms(),
                direction: Dir::In,
                name: "log".to_string(),
                payload: Value::String(s),
                ok: None,
                context: String::new(),
            });
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
