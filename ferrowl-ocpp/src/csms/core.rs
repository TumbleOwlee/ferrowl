//! The CSMS per-connection duplex loop (one task per accepted CS).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::action_handler::CsmsActionHandler;
use super::command::ConnCommand;
use super::registry::{ConnectionId, ConnectionRegistry};
use crate::action::Version;
use crate::conn::{Connection, InboundDispatch};
use crate::error::CallError;
use crate::log::LogFn;

/// Bridges the role-agnostic [`InboundDispatch`] to the user's [`CsmsActionHandler`], binding the
/// originating [`ConnectionId`] so the handler can tell connections apart.
pub(crate) struct CsmsDispatch<V: Version, H: CsmsActionHandler<V>> {
    handler: Arc<H>,
    conn: ConnectionId,
    _v: std::marker::PhantomData<fn() -> V>,
}

impl<V: Version, H: CsmsActionHandler<V>> InboundDispatch<V> for CsmsDispatch<V, H> {
    fn handle(
        &self,
        action: V::Action,
    ) -> impl Future<Output = Result<V::Response, CallError>> + Send {
        self.handler.handle_call(self.conn, action)
    }
}

/// Run one accepted CS connection until termination, channel close, or peer disconnect, then
/// deregister it.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_connection<V, H, S, L>(
    ws: S,
    handler: Arc<H>,
    conn: ConnectionId,
    mut commands: mpsc::Receiver<ConnCommand<V>>,
    registry: Arc<ConnectionRegistry<V>>,
    log: L,
    timeout: Duration,
) where
    V: Version,
    H: CsmsActionHandler<V>,
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures_util::Sink<Message>
        + Send
        + 'static,
    S::Error: Send,
    L: LogFn + Clone,
{
    let dispatch = Arc::new(CsmsDispatch {
        handler: handler.clone(),
        conn,
        _v: std::marker::PhantomData,
    });
    let connection = Connection::<V>::start(ws, dispatch, log.clone(), timeout);
    handler.on_connected(conn).await;

    let shutdown = connection.shutdown.clone();
    let notified = shutdown.notified();
    tokio::pin!(notified);

    loop {
        tokio::select! {
            _ = &mut notified => break,
            cmd = commands.recv() => match cmd {
                None | Some(ConnCommand::Terminate) => break,
                Some(ConnCommand::Fire(action)) => {
                    if let Err(e) = connection.outbound.fire(action).await {
                        log.invoke(format!("CSMS {conn} failed to send action: {e}")).await;
                    }
                }
                Some(ConnCommand::Call(action, reply_tx)) => {
                    connection.outbound.call(action, reply_tx).await;
                }
            },
        }
    }

    connection.shutdown().await;
    handler.on_disconnected(conn).await;
    registry.remove(conn);
}
