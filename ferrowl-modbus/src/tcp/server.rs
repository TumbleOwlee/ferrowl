// Crate
use crate::server_core::Server;
use crate::tcp::Config;
use crate::tcp::tls::{map_tls_err, read_pem};
use crate::{Error, Key, KeyParams, LogFn, TcpError};

// Workspace
use ferrowl_store::Memory;

// External
use parking_lot::RwLock as MemLock;
use rust_modbus::{
    ClientCertPolicy, RootStore, Server as ModbusServer, TcpListener, TlsListener, TlsServerConfig,
};
use rustls_pki_types::{CertificateDer, PrivateKeyDer};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Builds and spawns a Modbus TCP server task answering requests from the
/// shared `memory`.
pub struct ServerBuilder<T: KeyParams> {
    config: Arc<RwLock<Config>>,
    memory: Arc<MemLock<Memory<Key<T>>>>,
}

impl<T: KeyParams> ServerBuilder<T> {
    pub fn new(config: Arc<RwLock<Config>>, memory: Arc<MemLock<Memory<Key<T>>>>) -> Self {
        Self { config, memory }
    }

    /// Binds the configured listen address and spawns the accept loop as a
    /// tokio task. `log` receives log lines.
    pub async fn spawn<L>(&self, log: L) -> Result<JoinHandle<Result<(), Error>>, Error>
    where
        L: LogFn + Clone,
    {
        let guard = self.config.read().await;
        run(&guard, self.memory.clone(), log).await
    }
}

/// Resolve the server's presented certificate per MB-R-106/107, and whether an
/// ephemeral self-signed certificate was used *without* being explicitly
/// requested (the caller logs that case). Explicit `cert_file`/`key_file` always
/// win over `self_signed` when both are set (edge-cases.md "TLS boundaries").
fn resolve_server_identity(
    cfg: &crate::tcp::ModbusTlsConfig,
    bind_host: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>, bool), TcpError> {
    match (&cfg.cert_file, &cfg.key_file) {
        (Some(cert), Some(key)) => {
            let chain = rust_modbus::load_pem_cert_chain(&read_pem(cert)?).map_err(map_tls_err)?;
            // A PEM document with no certificate blocks at all (garbage input) parses
            // successfully to an empty chain rather than erroring; catch that here so it
            // fails at the same TLS-configuration-error tier as every other malformed-PEM
            // case (edge-cases.md "TLS boundaries"), rather than surfacing later, at TLS
            // listener bind time, as a bare `Error::Server(Error::TlsHandshake)`.
            if chain.is_empty() {
                return Err(TcpError::Configuration(format!(
                    "{cert} contains no certificate"
                )));
            }
            let k = rust_modbus::load_pem_private_key(&read_pem(key)?).map_err(map_tls_err)?;
            Ok((chain, k, false))
        }
        (None, None) => {
            let (chain, k) = generate_self_signed(bind_host)?;
            Ok((chain, k, !cfg.self_signed))
        }
        _ => Err(TcpError::Configuration(
            "cert_file and key_file must both be set, or neither (MB-R-107)".into(),
        )),
    }
}

/// An ephemeral self-signed certificate/key pair, generated fresh in memory and
/// never written to disk (MB-R-106 fallback). `host` and `"localhost"` (when
/// different) are carried as SAN entries, mirroring
/// `ferrowl-ocpp/src/security.rs::generate_self_signed`.
fn generate_self_signed(
    host: &str,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>), TcpError> {
    let mut names = vec![host.to_string()];
    if host != "localhost" {
        names.push("localhost".to_string());
    }
    let mut params = rcgen::CertificateParams::new(names)
        .map_err(|e| TcpError::Configuration(format!("self-signed cert generation failed: {e}")))?;
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "ferrowl Modbus");
    let key_pair = rcgen::KeyPair::generate()
        .map_err(|e| TcpError::Configuration(format!("self-signed key generation failed: {e}")))?;
    let cert = params
        .self_signed(&key_pair)
        .map_err(|e| TcpError::Configuration(format!("self-signed cert generation failed: {e}")))?;
    let cert_der = cert.der().clone();
    let key_der = PrivateKeyDer::try_from(key_pair.serialize_der())
        .map_err(|e| TcpError::Configuration(format!("self-signed key encoding failed: {e}")))?;
    Ok((vec![cert_der], key_der))
}

/// Build the full `rust_modbus` server TLS config, and whether the self-signed
/// fallback was used without being requested (the caller logs that case).
fn build_server_tls_config(
    cfg: &crate::tcp::ModbusTlsConfig,
    bind_host: &str,
) -> Result<(TlsServerConfig, bool), TcpError> {
    let (cert_chain, key, used_fallback) = resolve_server_identity(cfg, bind_host)?;
    let client_certs = if cfg.require_client_cert {
        let ca = cfg.client_ca_file.as_ref().ok_or_else(|| {
            TcpError::Configuration(
                "require_client_cert is set but no client_ca_file was configured (MB-R-108)".into(),
            )
        })?;
        let mut roots = RootStore::empty();
        roots.add_pem(&read_pem(ca)?).map_err(map_tls_err)?;
        ClientCertPolicy::Require(roots)
    } else {
        ClientCertPolicy::None
    };
    Ok((
        TlsServerConfig {
            cert_chain,
            key,
            client_certs,
        },
        used_fallback,
    ))
}

/// Bind the configured TCP address and spawn the accept loop; each accepted connection answers from
/// the shared `memory` via a [`Server`] (verbose logging on). Plain TCP unless `config.tls` is set
/// (MB-R-104), in which case the listener terminates TLS on each accepted connection.
async fn run<T, L>(
    config: &Config,
    memory: Arc<MemLock<Memory<Key<T>>>>,
    log: L,
) -> Result<JoinHandle<Result<(), Error>>, Error>
where
    T: KeyParams,
    L: LogFn + Clone,
{
    let addr: SocketAddr = format!("{}:{}", config.ip, config.port)
        .parse()
        .map_err(|e| Error::Tcp(TcpError::Address(e)))?;
    // One service instance answers every accepted connection, so all of them share the
    // one store (MB-R-070). TCP servers log per-request outcomes (verbose = true).
    let server = ModbusServer::new(Server::new(memory, log.clone(), true));
    match &config.tls {
        None => match TcpListener::bind(addr).await {
            Ok(listener) => Ok(tokio::task::spawn(async move {
                server.serve(listener).await.map_err(Error::Server)
            })),
            Err(e) => Err(Error::Server(e)),
        },
        Some(tls) => {
            let (tls_config, used_fallback) =
                build_server_tls_config(tls, &config.ip).map_err(Error::Tcp)?;
            if used_fallback {
                log.invoke(
                    "No cert_file/key_file/self_signed configured for this TLS \
                     server; falling back to an ephemeral self-signed certificate."
                        .to_string(),
                )
                .await;
            }
            match TlsListener::bind(addr, tls_config).await {
                Ok(listener) => Ok(tokio::task::spawn(async move {
                    server
                        .serve_tls::<rust_modbus::Tcp>(listener)
                        .await
                        .map_err(Error::Server)
                })),
                Err(e) => Err(Error::Server(e)),
            }
        }
    }
}
