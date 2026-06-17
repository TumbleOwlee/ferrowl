//! CS = Charging Station (client role; dials out to a CSMS).

mod action_handler;
mod adapter;
mod command;
mod config;
mod core;
mod handler;
mod ops;

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;

use crate::action::Version;
use crate::error::{Error, WsError};
use crate::log::LogFn;

pub use action_handler::CsActionHandler;
pub use adapter::SemanticAdapter;
pub use command::Command;
pub use config::Config;
pub use handler::CsHandler;
pub use ops::CsOps;

/// Capacity of the command channel between a [`Client`] handle and its task.
const COMMAND_CHANNEL_CAP: usize = 32;

/// Adapter-internal transaction state (Decision 6): the v2.0.1 semantic `TransactionEvent` merge
/// needs a per-transaction `seq_no` counter that v1.6's flat messages don't carry. Held on the
/// client; harmless and unused for v1.6.
#[derive(Default)]
#[cfg_attr(not(feature = "v2_0_1"), allow(dead_code))]
pub(crate) struct TxState {
    next_tx: AtomicU64,
    seqs: Mutex<HashMap<String, i32>>,
}

#[cfg_attr(not(feature = "v2_0_1"), allow(dead_code))]
impl TxState {
    /// Mint a fresh transaction id and seed its sequence number at 0.
    fn start_transaction(&self) -> (String, i32) {
        let id = format!(
            "ferrowl-tx-{}",
            self.next_tx.fetch_add(1, Ordering::Relaxed)
        );
        self.seqs.lock().unwrap().insert(id.clone(), 0);
        (id, 0)
    }

    /// Next sequence number for an existing transaction (0 if previously unseen).
    fn next_seq(&self, id: &str) -> i32 {
        let mut guard = self.seqs.lock().unwrap();
        let entry = guard.entry(id.to_owned()).or_insert(-1);
        *entry += 1;
        *entry
    }
}

/// Builds and connects a CS client for a specific OCPP [`Version`].
pub struct ClientBuilder<V: Version> {
    config: Config,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> ClientBuilder<V> {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            _v: PhantomData,
        }
    }

    /// Dial the configured CSMS (advertising `V::subprotocol()`) and spawn the client task.
    ///
    /// `handler` answers CSMS-initiated Calls. For the low-level API pass a [`CsActionHandler`];
    /// for the semantic API pass [`SemanticAdapter::new(your_cs_handler)`](SemanticAdapter).
    pub async fn spawn<H, L>(self, handler: H, log: L) -> Result<Client<V>, Error>
    where
        H: CsActionHandler<V>,
        L: LogFn + Clone,
    {
        let mut request = self
            .config
            .url
            .as_str()
            .into_client_request()
            .map_err(WsError::from)?;
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_static(V::subprotocol()),
        );
        let (ws, _response) = connect_async(request).await.map_err(WsError::from)?;

        let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAP);
        let handle = tokio::spawn(core::run::<V, H, _, _>(
            ws,
            Arc::new(handler),
            cmd_rx,
            log,
            self.config.timeout(),
        ));

        Ok(Client {
            cmd_tx,
            handle: Some(handle),
            tx_state: Arc::new(TxState::default()),
            _v: PhantomData,
        })
    }
}

/// A handle to a running CS client task. Send [`Command`]s, or use the semantic [`CsOps`] methods.
pub struct Client<V: Version> {
    cmd_tx: mpsc::Sender<Command<V>>,
    handle: Option<JoinHandle<Result<(), Error>>>,
    #[cfg_attr(not(feature = "v2_0_1"), allow(dead_code))]
    pub(crate) tx_state: Arc<TxState>,
    _v: PhantomData<fn() -> V>,
}

impl<V: Version> Client<V> {
    /// Clone of the command sender, for drivers that want to hold their own.
    pub fn sender(&self) -> mpsc::Sender<Command<V>> {
        self.cmd_tx.clone()
    }

    /// Send a raw command to the client task.
    pub async fn send(&self, command: Command<V>) -> Result<(), Error> {
        self.cmd_tx
            .send(command)
            .await
            .map_err(|_| Error::ChannelClosed)
    }

    /// Send a Call and await its typed reply. A peer rejection is surfaced as [`Error::Call`].
    pub async fn call(&self, action: V::Action) -> Result<V::Response, Error> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.send(Command::SendActionAwait(action, reply_tx))
            .await?;
        match reply_rx.await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(call_err)) => Err(Error::Call(call_err)),
            Err(_) => Err(Error::ChannelClosed),
        }
    }

    /// Send a Call without awaiting its reply.
    pub async fn notify(&self, action: V::Action) -> Result<(), Error> {
        self.send(Command::SendAction(action)).await
    }

    /// Terminate the client task and wait for it to finish.
    pub async fn terminate(mut self) -> Result<(), Error> {
        let _ = self.cmd_tx.send(Command::Terminate).await;
        self.join().await
    }

    /// Wait for the client task to finish.
    pub async fn join(&mut self) -> Result<(), Error> {
        match self.handle.take() {
            Some(handle) => handle.await.map_err(|_| Error::NotRunning)?,
            None => Ok(()),
        }
    }
}
