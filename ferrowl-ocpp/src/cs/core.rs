//! The CS-side duplex loop: drives the command channel against a shared [`Connection`].

use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use super::Command;
use super::action_handler::CsActionHandler;
use crate::action::Version;
use crate::conn::{Connection, InboundDispatch};
use crate::error::{CallError, Error};
use crate::log::LogFn;

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
pub(crate) async fn run<V, H, S, L>(
    ws: S,
    handler: Arc<H>,
    mut commands: mpsc::Receiver<Command<V>>,
    log: L,
    timeout: Duration,
) -> Result<(), Error>
where
    V: Version,
    H: CsActionHandler<V>,
    S: futures_util::Stream<Item = Result<Message, tokio_tungstenite::tungstenite::Error>>
        + futures_util::Sink<Message>
        + Send
        + 'static,
    S::Error: Send,
    L: LogFn + Clone,
{
    let dispatch = Arc::new(CsDispatch {
        handler: handler.clone(),
        _v: PhantomData,
    });
    let connection = Connection::<V>::start(ws, dispatch, log.clone(), timeout);
    handler.on_connected().await;

    let shutdown = connection.shutdown.clone();
    let notified = shutdown.notified();
    tokio::pin!(notified);

    loop {
        tokio::select! {
            _ = &mut notified => break,
            cmd = commands.recv() => match cmd {
                None | Some(Command::Terminate) => break,
                Some(Command::SendAction(action)) => {
                    if let Err(e) = connection.outbound.fire(action).await {
                        log.invoke(format!("CS failed to send action: {e}")).await;
                    }
                }
                Some(Command::SendActionAwait(action, reply_tx)) => {
                    connection.outbound.call(action, reply_tx).await;
                }
            },
        }
    }

    connection.shutdown().await;
    handler.on_disconnected().await;
    Ok(())
}
