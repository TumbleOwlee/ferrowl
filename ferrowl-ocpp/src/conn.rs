//! The shared duplex connection engine used by both CS and CSMS.
//!
//! OCPP-J is bidirectional on a single socket, so a connection is driven by three concurrent
//! pieces (Decision 2 in the design plan):
//!
//! * a **writer task** that owns the websocket sink and serializes every outbound frame fed to it
//!   over an mpsc channel;
//! * a **reader task** that owns the websocket stream, completes correlated replies, and — for each
//!   inbound `Call` — **spawns a handler task** so a slow or re-entrant handler never blocks the
//!   read pump; and
//! * an [`OutboundHandle`] the role-specific command loop uses to send Calls, where awaiting a
//!   reply happens in a **per-call task** that owns the originating action and decodes the reply.

use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{Notify, mpsc, oneshot};
use tokio_tungstenite::tungstenite::Message;

use crate::action::Version;
use crate::correlation::PendingCalls;
use crate::error::{CallError, Error};
use crate::log::LogFn;
use crate::ocppj::{CallErrorCode, OcppJMessage, UniqueId, codec};

/// Capacity of the outbound frame channel feeding the writer task.
const OUTBOUND_CHANNEL_CAP: usize = 64;

/// Role-agnostic inbound dispatch: turn a decoded action into a response future. CS implements
/// this directly over its handler; CSMS binds the originating [`ConnectionId`](crate::csms) first.
pub(crate) trait InboundDispatch<V: Version>: Send + Sync + 'static {
    fn handle(
        &self,
        action: V::Action,
    ) -> impl Future<Output = Result<V::Response, CallError>> + Send;
}

/// Handle the role-specific command loop uses to push outbound Calls onto the connection.
pub(crate) struct OutboundHandle<V: Version> {
    out_tx: mpsc::Sender<OcppJMessage>,
    pending: PendingCalls,
    timeout: Duration,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> OutboundHandle<V> {
    /// Send a Call without awaiting its reply.
    pub(crate) async fn fire(&self, action: V::Action) -> Result<(), Error> {
        let payload = V::encode_action(&action)?;
        let msg = OcppJMessage::Call {
            id: UniqueId::generate(),
            action: V::action_name(&action).to_owned(),
            payload,
        };
        self.out_tx
            .send(msg)
            .await
            .map_err(|_| Error::ChannelClosed)
    }

    /// Send a Call and arrange for `reply_tx` to be fulfilled with the typed, decoded reply. The
    /// wait happens in a spawned task that owns `action` (needed to decode the otherwise
    /// action-less `CallResult` payload), so the caller's command loop is never blocked.
    pub(crate) async fn call(
        &self,
        action: V::Action,
        reply_tx: oneshot::Sender<Result<V::Response, CallError>>,
    ) {
        let payload = match V::encode_action(&action) {
            Ok(p) => p,
            Err(e) => {
                let _ = reply_tx.send(Err(e.into()));
                return;
            }
        };
        let id = UniqueId::generate();
        let rx = self.pending.register(id.clone());
        let msg = OcppJMessage::Call {
            id: id.clone(),
            action: V::action_name(&action).to_owned(),
            payload,
        };
        if self.out_tx.send(msg).await.is_err() {
            self.pending.remove(&id);
            let _ = reply_tx.send(Err(CallError::new(
                CallErrorCode::GenericError,
                "connection closed",
            )));
            return;
        }

        let pending = self.pending.clone();
        let timeout = self.timeout;
        tokio::spawn(async move {
            let outcome = match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(Ok(value))) => V::decode_result(&action, value).map_err(CallError::from),
                Ok(Ok(Err(call_err))) => Err(call_err),
                Ok(Err(_)) => Err(CallError::new(
                    CallErrorCode::GenericError,
                    "reply channel dropped",
                )),
                Err(_) => {
                    pending.remove(&id);
                    Err(CallError::new(
                        CallErrorCode::GenericError,
                        "call timed out",
                    ))
                }
            };
            let _ = reply_tx.send(outcome);
        });
    }
}

/// All the moving parts of a live connection, handed to a role's command loop.
///
/// The `pending` correlation map and the outbound sender live inside [`OutboundHandle`]; the
/// teardown path reaches them through `outbound` rather than keeping its own copies.
pub(crate) struct Connection<V: Version> {
    pub(crate) outbound: OutboundHandle<V>,
    pub(crate) shutdown: Arc<Notify>,
    writer: tokio::task::JoinHandle<()>,
    reader: tokio::task::JoinHandle<()>,
}

impl<V: Version> Connection<V> {
    /// Split `ws` and spawn its writer and reader tasks. `dispatch` answers inbound Calls.
    pub(crate) fn start<S, D, L>(ws: S, dispatch: Arc<D>, log: L, timeout: Duration) -> Self
    where
        S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
            + futures_util::Sink<Message>
            + Send
            + 'static,
        S::Error: Send,
        D: InboundDispatch<V>,
        L: LogFn + Clone,
    {
        let (sink, stream) = ws.split();
        let (out_tx, out_rx) = mpsc::channel::<OcppJMessage>(OUTBOUND_CHANNEL_CAP);
        let pending = PendingCalls::new();
        let shutdown = Arc::new(Notify::new());

        let writer = tokio::spawn(writer_task(sink, out_rx));
        let reader = tokio::spawn(reader_task::<V, _, D, L>(
            stream,
            out_tx.clone(),
            pending.clone(),
            dispatch,
            log,
            shutdown.clone(),
        ));

        // `out_tx`/`pending` move into the handle; the reader keeps its own clones.
        let outbound = OutboundHandle {
            out_tx,
            pending,
            timeout,
            _v: PhantomData,
        };

        Self {
            outbound,
            shutdown,
            writer,
            reader,
        }
    }

    /// Tear the connection down: stop the reader, fail every pending call, and drain the writer.
    pub(crate) async fn shutdown(self) {
        self.reader.abort();
        self.outbound.pending.fail_all(&CallError::new(
            CallErrorCode::GenericError,
            "connection terminated",
        ));
        // Dropping the handle drops the last live outbound sender; with the reader aborted, the
        // writer's channel then closes and the writer task drains and exits.
        drop(self.outbound);
        let _ = self.writer.await;
    }
}

/// Owns the websocket sink; writes every outbound frame fed over the channel until it closes.
async fn writer_task<Si>(mut sink: Si, mut out_rx: mpsc::Receiver<OcppJMessage>)
where
    Si: futures_util::Sink<Message> + Unpin,
{
    while let Some(msg) = out_rx.recv().await {
        if sink.send(codec::encode(&msg)).await.is_err() {
            break;
        }
    }
    let _ = sink.close().await;
}

/// Owns the websocket stream; completes correlated replies and spawns a task per inbound Call.
async fn reader_task<V, St, D, L>(
    mut stream: St,
    out_tx: mpsc::Sender<OcppJMessage>,
    pending: PendingCalls,
    dispatch: Arc<D>,
    log: L,
    shutdown: Arc<Notify>,
) where
    V: Version,
    St: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    D: InboundDispatch<V>,
    L: LogFn + Clone,
{
    while let Some(item) = stream.next().await {
        let msg = match item {
            Ok(Message::Text(text)) => match codec::decode(text.as_str()) {
                Ok(msg) => msg,
                Err(e) => {
                    log.invoke(format!("OCPP-J framing error: {e}")).await;
                    // A malformed Call whose id survives is still owed an answer -- without one
                    // the peer waits out its own call timeout. Anything else (unparseable text,
                    // no id, or a malformed CallResult/CallError) has no one to answer.
                    if let Some(id) = codec::recover_call_id(text.as_str()) {
                        let _ = out_tx
                            .send(OcppJMessage::CallError {
                                id,
                                code: CallErrorCode::FormationViolation,
                                description: e.to_string(),
                                details: serde_json::Value::Object(serde_json::Map::new()),
                            })
                            .await;
                    }
                    continue;
                }
            },
            Ok(Message::Close(_)) => break,
            Ok(_) => continue, // ping/pong/binary/raw frames are not OCPP-J payloads
            Err(e) => {
                log.invoke(format!("websocket error: {e}")).await;
                break;
            }
        };

        match msg {
            OcppJMessage::Call {
                id,
                action,
                payload,
            } => {
                let dispatch = dispatch.clone();
                let out_tx = out_tx.clone();
                tokio::spawn(async move {
                    let reply = dispatch_call::<V, D>(&dispatch, id, action, payload).await;
                    let _ = out_tx.send(reply).await;
                });
            }
            OcppJMessage::CallResult { id, payload } => pending.complete(&id, Ok(payload)),
            OcppJMessage::CallError {
                id,
                code,
                description,
                details,
            } => pending.complete(
                &id,
                Err(CallError {
                    code,
                    description,
                    details,
                }),
            ),
        }
    }
    // `notify_one` stores a permit if the command loop hasn't parked on the future yet, so the
    // shutdown signal is never lost to a race.
    shutdown.notify_one();
}

/// Decode -> validate -> dispatch a single inbound Call, producing the frame to send back.
async fn dispatch_call<V, D>(
    dispatch: &D,
    id: UniqueId,
    action_name: String,
    payload: serde_json::Value,
) -> OcppJMessage
where
    V: Version,
    D: InboundDispatch<V>,
{
    let into_error = |id: UniqueId, err: CallError| OcppJMessage::CallError {
        id,
        code: err.code,
        description: err.description,
        details: err.details,
    };

    let action = match V::decode_call(&action_name, payload) {
        Ok(action) => action,
        Err(e) => return into_error(id, e.into()),
    };
    if let Err(ve) = V::validate(&action) {
        return into_error(
            id,
            CallError::new(CallErrorCode::FormationViolation, ve.to_string()),
        );
    }
    match dispatch.handle(action).await {
        Ok(response) => match V::encode_response(&response) {
            Ok(payload) => OcppJMessage::CallResult { id, payload },
            Err(e) => into_error(id, e.into()),
        },
        Err(call_err) => into_error(id, call_err),
    }
}
