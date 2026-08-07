use crate::client_core::{ClientCore, ConnectAttempt};
use crate::tcp::Config;
use crate::tcp::tls::{map_tls_err, read_pem};
use crate::{Command, Error, Key, KeyParams, LogFn, Operation, TcpError};

use ferrowl_store::Memory;
use parking_lot::RwLock as MemLock;
use rust_modbus::{
    Client as ModbusClient, ClientIdentity, FrameTransport, RootStore, ServerCertVerification,
    TcpConfig, TlsClientConfig, connect_tcp, connect_tls, load_pem_cert_chain,
    load_pem_private_key,
};
use tokio::task::JoinHandle;

use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::RwLock;
use tokio::sync::mpsc::Receiver;

/// Builds and spawns a Modbus TCP client task that polls `operations` into
/// the shared `memory` and executes incoming [`Command`]s.
pub struct ClientBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    operations: Arc<RwLock<Vec<Operation>>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ClientBuilder<T> {
    pub fn new(
        config: Arc<RwLock<Config>>,
        operations: Arc<RwLock<Vec<Operation>>>,
        memory: Arc<MemLock<Memory<Key<T>>>>,
    ) -> Self {
        Self {
            config,
            operations,
            memory,
        }
    }

    /// Connects to the configured endpoint and spawns the client loop as a tokio task. `log`
    /// receives log lines, `status` receives connection status updates, and `receiver` delivers
    /// write/terminate [`Command`]s.
    ///
    /// With `config.reconnect` set (the default), a lost or refused connection does not end the
    /// task: it logs, waits an exponential backoff (capped, reset after a run that got at least
    /// one read through), and reconnects. `Command::Terminate` (or the channel closing) aborts a
    /// backoff wait immediately. With `config.reconnect` unset, a transport error ends the task
    /// exactly as before this behavior was added.
    pub async fn spawn<L, S>(
        &self,
        receiver: Receiver<Command>,
        log: L,
        status: S,
    ) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
        S: LogFn + Clone,
    {
        let config = self.config.clone();
        let operations = self.operations.clone();
        let memory = self.memory.clone();
        Ok(tokio::task::spawn(async move {
            ClientCore::run_reconnect_loop(receiver, log, status, operations, memory, move || {
                let config = config.clone();
                async move {
                    let guard = config.read().await;
                    let attempt = ConnectAttempt {
                        reconnect: guard.reconnect,
                        timeout_ms: guard.timeout_ms,
                        delay_ms: guard.delay_ms,
                        interval_ms: guard.interval_ms,
                        client: Client::connect(&guard).await.map(|client| client.core),
                    };
                    drop(guard);
                    attempt
                }
            })
            .await
        }))
    }
}

/// A client-side socket that is either plain TCP or TLS-terminated TCP (MB-R-104).
pub(crate) enum ClientStream {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<tokio::net::TcpStream>>),
}

impl AsyncRead for ClientStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ClientStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_flush(cx),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ClientStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            ClientStream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Build the `rust_modbus` client TLS config from ours: native roots plus `ca_file`
/// if set, unless `insecure_skip_verify` is set (in which case `ca_file` is ignored
/// and no server certificate is checked at all) — MB-R-109. A client identity is
/// presented only when both `client_cert_file` and `client_key_file` are set;
/// either alone presents nothing — MB-R-110.
fn build_client_tls_config(cfg: &crate::tcp::ModbusTlsConfig) -> Result<TlsClientConfig, TcpError> {
    let server_cert = if cfg.insecure_skip_verify {
        ServerCertVerification::DangerousDisableVerification
    } else {
        let mut roots = RootStore::native();
        if let Some(path) = &cfg.ca_file {
            roots.add_pem(&read_pem(path)?).map_err(map_tls_err)?;
        }
        ServerCertVerification::Verify(roots)
    };
    let client_identity = match (&cfg.client_cert_file, &cfg.client_key_file) {
        (Some(cert), Some(key)) => Some(ClientIdentity {
            cert_chain: load_pem_cert_chain(&read_pem(cert)?).map_err(map_tls_err)?,
            key: load_pem_private_key(&read_pem(key)?).map_err(map_tls_err)?,
        }),
        _ => None,
    };
    Ok(TlsClientConfig {
        server_cert,
        client_identity,
    })
}

/// A connected Modbus TCP client. Connection setup is TCP-specific; the read/command loop is
/// shared via the internal `ClientCore`, over a socket carrying Modbus TCP framing.
pub struct Client {
    pub(crate) core: ClientCore<ClientStream, rust_modbus::Tcp>,
}

impl Client {
    /// Opens a TCP connection to `config.ip:config.port`, bounded by the
    /// configured timeout. Plain TCP unless `config.tls` is set (MB-R-104), in which
    /// case the same timeout bounds the TCP connect and the TLS handshake together.
    pub async fn connect(config: &Config) -> Result<Self, Error> {
        let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
            .parse()
            .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
        let tls_config = config
            .tls
            .as_ref()
            .map(build_client_tls_config)
            .transpose()?;
        let attempt = async {
            match tls_config {
                None => connect_tcp(addr, TcpConfig::default())
                    .await
                    .map(|t| ClientStream::Plain(t.into_inner())),
                Some(tls) => connect_tls(addr, TcpConfig::default(), tls)
                    .await
                    .map(|t| ClientStream::Tls(Box::new(t.into_inner()))),
            }
        };
        match tokio::time::timeout(
            std::time::Duration::from_millis(config.timeout_ms as u64),
            attempt,
        )
        .await
        {
            Ok(Ok(stream)) => Ok(Self {
                core: ClientCore {
                    client: ModbusClient::<_, _>::new(FrameTransport::new(stream)),
                },
            }),
            Ok(Err(e)) => Err(TcpError::Error(e).into()),
            Err(e) => Err(TcpError::Timeout(e).into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::build_client_tls_config;
    use crate::tcp::ModbusTlsConfig;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn write_pem(label: &str, pem: &str) -> String {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ferrowl-modbus-client-tls-ut-{}-{label}-{n}.pem",
            std::process::id()
        ));
        std::fs::write(&path, pem).expect("write test pem");
        path.to_string_lossy().into_owned()
    }

    fn cert_and_key_pem() -> (String, String) {
        let key = rcgen::KeyPair::generate().expect("keypair generation failed");
        let cert = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .expect("cert params")
            .self_signed(&key)
            .expect("self-signed cert");
        (cert.pem(), key.serialize_pem())
    }

    /// MB-R-110 — a client identity is presented only when both `client_cert_file` and
    /// `client_key_file` are set.
    #[test]
    fn ut_build_client_tls_config_identity_only_when_both_files_set() {
        let (cert_pem, key_pem) = cert_and_key_pem();
        let cert_file = write_pem("cert", &cert_pem);
        let key_file = write_pem("key", &key_pem);

        let both = build_client_tls_config(&ModbusTlsConfig {
            client_cert_file: Some(cert_file.clone()),
            client_key_file: Some(key_file.clone()),
            ..Default::default()
        })
        .expect("builds");
        assert!(both.client_identity.is_some());

        let cert_only = build_client_tls_config(&ModbusTlsConfig {
            client_cert_file: Some(cert_file),
            ..Default::default()
        })
        .expect("builds");
        assert!(cert_only.client_identity.is_none());

        let key_only = build_client_tls_config(&ModbusTlsConfig {
            client_key_file: Some(key_file),
            ..Default::default()
        })
        .expect("builds");
        assert!(key_only.client_identity.is_none());

        let neither = build_client_tls_config(&ModbusTlsConfig::default()).expect("builds");
        assert!(neither.client_identity.is_none());
    }

    /// MB-R-109 — `insecure_skip_verify` disables server certificate verification and
    /// ignores `ca_file`, rather than combining the two.
    #[test]
    fn ut_build_client_tls_config_insecure_skip_verify_ignores_ca_file() {
        use rust_modbus::ServerCertVerification;

        let cfg = ModbusTlsConfig {
            ca_file: Some("/no/such/ca.pem".to_string()),
            insecure_skip_verify: true,
            ..Default::default()
        };
        let built = build_client_tls_config(&cfg).expect("builds despite unreadable ca_file");
        assert!(matches!(
            built.server_cert,
            ServerCertVerification::DangerousDisableVerification
        ));
    }
}
