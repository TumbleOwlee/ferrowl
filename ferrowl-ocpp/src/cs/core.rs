//! The CS-side duplex loop: drives the command channel against a shared [`Connection`].

use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use tokio_tungstenite::tungstenite::Message;

use super::Command;
use super::action_handler::CsActionHandler;
use crate::action::Version;
use crate::conn::{Connection, InboundDispatch};
use crate::error::CallError;
use crate::log::LogFn;

/// How a completed connection ended, distinguishing an explicit stop (OC-R-106) from a dropped
/// connection (OC-R-048) — the two must be classified differently by the retry loop in
/// `ClientBuilder::spawn`: `Terminated` stops retrying, `Disconnected` triggers a backoff+retry
/// (or ends the task with an error, per `reconnect`).
pub(crate) enum RunEnd {
    /// `Command::Terminate` was received, or the command channel closed (every sender dropped).
    Terminated,
    /// The connection's reader task observed the peer close, a websocket error, or a stream end.
    Disconnected,
}

/// Bridges the role-agnostic [`InboundDispatch`] to the user's [`CsActionHandler`].
pub(crate) struct CsDispatch<V: Version, H: CsActionHandler<V>> {
    handler: Arc<H>,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version, H: CsActionHandler<V>> InboundDispatch<V> for CsDispatch<V, H> {
    fn handle(
        &self,
        action: V::Action,
    ) -> impl Future<Output = Result<V::Response, CallError>> + Send {
        self.handler.handle_call(action)
    }
}

/// Run the CS connection until `Terminate`, channel close, or the peer disconnects.
pub(crate) async fn run<V, H, S, L, Stat>(
    ws: S,
    handler: Arc<H>,
    commands: &mut super::Commands<'_, Command<V>>,
    log: L,
    status: Stat,
    timeout: Duration,
) -> RunEnd
where
    V: Version,
    H: CsActionHandler<V>,
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures_util::Sink<Message>
        + Send
        + 'static,
    S::Error: Send,
    L: LogFn + Clone,
    Stat: LogFn + Clone,
{
    let dispatch = Arc::new(CsDispatch {
        handler: handler.clone(),
        _v: PhantomData,
    });
    let connection = Connection::<V>::start(ws, dispatch, status, timeout);
    handler.on_connected().await;

    let shutdown = connection.shutdown.clone();
    let notified = shutdown.notified();
    tokio::pin!(notified);

    let end = loop {
        tokio::select! {
            _ = &mut notified => break RunEnd::Disconnected,
            cmd = commands.recv() => match cmd {
                None | Some(Command::Terminate) => break RunEnd::Terminated,
                Some(Command::SendAction(action)) => {
                    if let Err(e) = connection.outbound.fire(action).await {
                        log.invoke(crate::Level::Error, format!("CS failed to send action: {e}")).await;
                    }
                }
                Some(Command::SendActionAwait(action, reply_tx)) => {
                    connection.outbound.call(action, reply_tx).await;
                }
            },
        }
    };

    connection.shutdown().await;
    handler.on_disconnected().await;
    end
}

#[cfg(all(test, feature = "v1_6"))]
mod tests {
    use super::*;
    use crate::{Action16, CallError, CallErrorCode, Response16, V1_6};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Error as WsError;

    /// A websocket that never yields a frame and rejects every write.
    struct DeadWs;

    impl futures_util::Stream for DeadWs {
        type Item = Result<Message, WsError>;
        fn poll_next(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Pending
        }
    }

    impl futures_util::Sink<Message> for DeadWs {
        type Error = WsError;
        fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), WsError>> {
            Poll::Ready(Ok(()))
        }
        fn start_send(self: Pin<&mut Self>, _: Message) -> Result<(), WsError> {
            Err(WsError::ConnectionClosed)
        }
        fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), WsError>> {
            Poll::Ready(Ok(()))
        }
        fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), WsError>> {
            Poll::Ready(Ok(()))
        }
    }

    struct Handler;

    impl CsActionHandler<V1_6> for Handler {
        async fn handle_call(&self, _: Action16) -> Result<Response16, CallError> {
            Err(CallError::new(CallErrorCode::NotImplemented, "unsupported"))
        }
    }

    /// OC-R-194 — an action that cannot be sent on the connection logs at Error.
    #[tokio::test]
    async fn ut_failed_send_logs_error() {
        let lines = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let sink = lines.clone();
        let log = move |level: crate::Level, s: String| {
            let sink = sink.clone();
            async move {
                sink.lock().push((level, s));
            }
        };
        let (tx, mut rx) = mpsc::channel(4);
        let mut commands = super::super::Commands::new(&mut rx, Default::default());
        let hb = || Action16::Heartbeat(serde_json::from_value(serde_json::json!({})).unwrap());
        let driver = async {
            // The first write kills the writer task; the next send then finds the channel closed.
            tx.send(Command::SendAction(hb())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            tx.send(Command::SendAction(hb())).await.unwrap();
            tokio::time::sleep(Duration::from_millis(100)).await;
            tx.send(Command::Terminate).await.unwrap();
        };
        let (end, ()) = tokio::join!(
            run::<V1_6, _, _, _, _>(
                DeadWs,
                Arc::new(Handler),
                &mut commands,
                log,
                |_: crate::Level, _: String| async {},
                Duration::from_secs(1),
            ),
            driver
        );
        let _ = end;
        let got = lines.lock().clone();
        assert!(
            got.iter().any(|(level, s): &(crate::Level, String)| {
                *level == crate::Level::Error && s.starts_with("CS failed to send action")
            }),
            "{got:?}"
        );
    }
}
