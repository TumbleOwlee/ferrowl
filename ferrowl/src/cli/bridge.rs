//! `ferrowl bridge`: relays Modbus requests between two configured interfaces. BR-R-001..015.
//!
//! Exit codes: `0` ran to completion (`--duration` elapsed or Ctrl-C), `1` `--upstream`/
//! `--downstream` missing or malformed, or the bridge failed to start (upstream bind/listen/
//! serial-open failure, BR-R-013), `3` `--exit-on-error` was set and a drained log line was
//! `[bridge]`-prefixed (a genuine relay failure).

use std::io::Write as _;
use std::time::{Duration, Instant};

use crate::cli::{BridgeArgs, parse_bridge_descriptor};
use crate::view::log::format_timestamp;

const SOURCE: &str = "bridge";
/// BR-R-013 — the bridge's own error-line prefix, unlike headless `run`'s `--exit-on-error`
/// (CL-R-031), which keys off log level rather than a prefix.
const ERROR_PREFIX: &str = ferrowl_modbus::bridge::ERROR_PREFIX;

/// Run the bridge described by `args`. Returns the process exit code; never panics on the
/// relay's own runtime errors (those surface as `[bridge]`-prefixed log lines), only on setup
/// failure (BR-R-013).
pub async fn run(args: &BridgeArgs) -> i32 {
    let (Some(upstream), Some(downstream)) = (args.upstream.as_deref(), args.downstream.as_deref())
    else {
        eprintln!("Error: --upstream and --downstream are both required");
        return 1;
    };
    let upstream = match parse_bridge_descriptor(upstream) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!("Error: invalid --upstream: {e}");
            return 1;
        }
    };
    let downstream = match parse_bridge_descriptor(downstream) {
        Ok(spec) => spec,
        Err(e) => {
            eprintln!("Error: invalid --downstream: {e}");
            return 1;
        }
    };

    let mut log_file = match crate::cli::open_log_file(args.log_file.as_deref()) {
        Ok(f) => f,
        Err((path, e)) => {
            eprintln!("Error: failed to open --log-file '{path}': {e}");
            return 1;
        }
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let log = move |msg: String| {
        let tx = tx.clone();
        async move {
            let _ = tx.send(msg);
        }
    };

    let handle = match ferrowl_modbus::bridge::run(
        ferrowl_modbus::bridge::BridgeConfig {
            upstream,
            downstream,
        },
        log,
    )
    .await
    {
        Ok(handle) => handle,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let deadline = args
        .duration
        .map(|secs| Instant::now() + Duration::from_secs(secs));
    let mut exit_code = 0;
    loop {
        tokio::select! {
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                let line = format!("[{}] {SOURCE} | {msg}", format_timestamp(crate::cli::now_ms()));
                println!("{line}");
                if let Some(f) = log_file.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
                if args.exit_on_error && msg.starts_with(ERROR_PREFIX) {
                    exit_code = 3;
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => break,
            _ = async {
                if let Some(d) = deadline {
                    tokio::time::sleep_until(d.into()).await
                } else {
                    std::future::pending().await
                }
            } => break,
        }
    }
    handle.abort();
    exit_code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::BridgeArgs;

    fn base_args() -> BridgeArgs {
        BridgeArgs {
            upstream: None,
            downstream: None,
            duration: Some(0),
            log_file: None,
            exit_on_error: false,
        }
    }

    /// BR-R-003, BR-R-013 — missing `--upstream` returns exit code 1.
    #[tokio::test]
    async fn ut_bridge_run_missing_upstream_or_downstream_returns_one() {
        let mut args = base_args();
        args.downstream = Some("transport=tcp,ip=127.0.0.1,port=1".to_string());
        assert_eq!(run(&args).await, 1);
    }

    /// BR-R-003, BR-R-013 — missing `--downstream` returns exit code 1.
    #[tokio::test]
    async fn ut_bridge_run_missing_downstream_returns_one() {
        let mut args = base_args();
        args.upstream = Some("transport=tcp,ip=127.0.0.1,port=1".to_string());
        assert_eq!(run(&args).await, 1);
    }

    /// BR-R-013 — a malformed `--upstream` descriptor returns exit code 1.
    #[tokio::test]
    async fn ut_bridge_run_invalid_descriptor_returns_one() {
        let mut args = base_args();
        args.upstream = Some("not-a-valid-descriptor".to_string());
        args.downstream = Some("transport=tcp,ip=127.0.0.1,port=1".to_string());
        assert_eq!(run(&args).await, 1);
    }

    // `ferrowl` is bin-only (no lib target `ferrowl/tests/` integration tests could call
    // `cli::bridge::run` from), same constraint `cli::headless::run` already lives under —
    // its own richer end-to-end/log-file/exit-on-error scenarios live here too, in-process,
    // rather than in `ferrowl/tests/it_bridge.rs` (which instead drives the compiled binary
    // as a subprocess, mirroring `tests/headless.rs`'s smoke tests).

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    async fn seeded_downstream_server(
        port: u16,
        value: u16,
    ) -> (
        tokio::sync::mpsc::Sender<ferrowl_modbus::ServerCommand>,
        tokio::task::JoinHandle<Result<(), ferrowl_modbus::Error>>,
    ) {
        use ferrowl_codec::Kind as RegKind;
        use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
        use parking_lot::RwLock as MemLock;
        use std::sync::Arc;

        let key = ferrowl_modbus::Key::new(ferrowl_modbus::SlaveKey {
            slave_id: ferrowl_modbus::UnitId(1),
            kind: RegKind::HoldingRegister,
        });
        let mut mem = Memory::<ferrowl_modbus::Key<ferrowl_modbus::SlaveKey>>::default();
        mem.add_ranges(
            key.clone(),
            &MemKind::read_write(CellType::Register),
            &[Range::new(0, 4)],
        );
        mem.write(key, &CellType::Register, &Range::new(0, 1), &[value])
            .unwrap();
        let srv_mem = Arc::new(MemLock::new(mem));

        let config = ferrowl_modbus::tcp::Config {
            ip: "127.0.0.1".to_string(),
            port,
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 0,
            reconnect: true,
            tls: None,
        };
        // The sender must be returned alongside the handle and kept alive by the caller for as
        // long as the server should keep running: the shared server core treats the command
        // channel closing (every sender dropped) the same as an explicit `Terminate` (MB-R-133),
        // so a sender dropped immediately after `spawn()` would end this task before the test
        // gets to use it.
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let (handle, _bound_addr) = ferrowl_modbus::tcp::ServerBuilder::new(
            Arc::new(tokio::sync::RwLock::new(config)),
            srv_mem,
            ferrowl_modbus::tcp::new_self_signed_cache(),
        )
        .spawn(
            receiver,
            |_s: String| async move {},
            |_s: String| async move {},
        )
        .await
        .expect("downstream server failed to start");
        (sender, handle)
    }

    fn descriptor(port: u16) -> String {
        format!("transport=tcp,ip=127.0.0.1,port={port}")
    }

    /// A `ClientBuilder` polling one holding-register read against `port`, writing results into
    /// `mem` — the same client-side pattern `ferrowl-modbus/tests/tcp_loopback.rs` uses to
    /// verify a server's answers, applied here against the bridge's upstream port.
    async fn poll_upstream(
        port: u16,
        mem: std::sync::Arc<
            parking_lot::RwLock<
                ferrowl_store::Memory<ferrowl_modbus::Key<ferrowl_modbus::SlaveKey>>,
            >,
        >,
    ) -> (
        tokio::sync::mpsc::Sender<ferrowl_modbus::Command>,
        tokio::task::JoinHandle<Result<(), ferrowl_modbus::Error>>,
    ) {
        use ferrowl_modbus::{Command, FunctionCode, Operation, UnitId, tcp};
        use ferrowl_store::Range;

        let config = tcp::Config {
            ip: "127.0.0.1".to_string(),
            port,
            timeout_ms: 1000,
            delay_ms: 0,
            interval_ms: 50,
            reconnect: true,
            tls: None,
        };
        let operations = std::sync::Arc::new(tokio::sync::RwLock::new(vec![Operation {
            slave_id: UnitId(1),
            fn_code: FunctionCode::ReadHoldingRegisters,
            range: Range::new(0, 1),
        }]));
        let (tx, rx) = tokio::sync::mpsc::channel::<Command>(4);
        let (handle, _connected) = tcp::ClientBuilder::new(
            std::sync::Arc::new(tokio::sync::RwLock::new(config)),
            operations,
            mem,
            ferrowl_modbus::tcp::new_self_signed_cache(),
        )
        .spawn(rx, |_s: String| async move {}, |_s: String| async move {})
        .await
        .expect("client failed to connect to upstream");
        (tx, handle)
    }

    fn client_key() -> ferrowl_modbus::Key<ferrowl_modbus::SlaveKey> {
        ferrowl_modbus::Key::new(ferrowl_modbus::SlaveKey {
            slave_id: ferrowl_modbus::UnitId(1),
            kind: ferrowl_codec::Kind::HoldingRegister,
        })
    }

    fn client_mem() -> std::sync::Arc<
        parking_lot::RwLock<ferrowl_store::Memory<ferrowl_modbus::Key<ferrowl_modbus::SlaveKey>>>,
    > {
        use ferrowl_store::{CellKind as MemKind, CellType, Memory, Range};
        let mut mem = Memory::<ferrowl_modbus::Key<ferrowl_modbus::SlaveKey>>::default();
        mem.add_ranges(
            client_key(),
            &MemKind::read_write(CellType::Register),
            &[Range::new(0, 4)],
        );
        std::sync::Arc::new(parking_lot::RwLock::new(mem))
    }

    /// BR-R-001..BR-R-007, BR-R-013 — a real downstream server is relayed through to by a real
    /// upstream bridge connection, and the run exits 0 on its `--duration` deadline
    /// (CL-R-032 family via BR-R-013).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_bridge_end_to_end_relays_a_real_request() {
        let downstream_port = free_port();
        let (_downstream_tx, _downstream) = seeded_downstream_server(downstream_port, 77).await;

        let upstream_port = free_port();
        let args = BridgeArgs {
            upstream: Some(descriptor(upstream_port)),
            downstream: Some(descriptor(downstream_port)),
            duration: Some(1),
            log_file: None,
            exit_on_error: false,
        };

        let mem = client_mem();
        let mem_for_poll = mem.clone();
        let poller = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            poll_upstream(upstream_port, mem_for_poll).await
        });
        let exit_code = run(&args).await;
        let (_tx, _handle) = poller.await.expect("poller task panicked");

        use ferrowl_store::{CellType, Range};
        let g = mem.read();
        assert_eq!(
            g.read(client_key(), &CellType::Register, &Range::new(0, 1))
                .unwrap(),
            vec![77]
        );
        assert_eq!(exit_code, 0);
    }

    /// BR-R-013 — with `--exit-on-error` set and no downstream listening, a forwarded request
    /// answers `GatewayPathUnavailable` and logs a `[bridge]`-prefixed line, making the run
    /// exit 3.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ut_bridge_exit_on_error_flags_bridge_prefixed_line() {
        // Nothing listens on this downstream port.
        let downstream_port = free_port();
        let upstream_port = free_port();
        let args = BridgeArgs {
            upstream: Some(descriptor(upstream_port)),
            downstream: Some(descriptor(downstream_port)),
            duration: Some(5),
            log_file: None,
            exit_on_error: true,
        };

        let mem = client_mem();
        let (_client, exit_code) = tokio::join!(
            async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                poll_upstream(upstream_port, mem).await
            },
            run(&args)
        );
        assert_eq!(exit_code, 3);
    }

    /// BR-R-012, CL-R-041 — `--log-file` is opened create-and-append: pre-existing content
    /// survives, new bridge log lines are appended.
    #[tokio::test]
    async fn ut_bridge_log_file_appended_not_truncated() {
        let log_file = std::env::temp_dir()
            .join("ferrowl_cl_bridge_append.log")
            .to_str()
            .unwrap()
            .to_string();
        std::fs::write(&log_file, "PREEXISTING\n").unwrap();

        let downstream_port = free_port();
        let (_downstream_tx, _downstream) = seeded_downstream_server(downstream_port, 5).await;
        let upstream_port = free_port();
        let args = BridgeArgs {
            upstream: Some(descriptor(upstream_port)),
            downstream: Some(descriptor(downstream_port)),
            duration: Some(1),
            log_file: Some(log_file.clone()),
            exit_on_error: false,
        };
        assert_eq!(run(&args).await, 0);

        let contents = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            contents.starts_with("PREEXISTING\n"),
            "the pre-existing content must be preserved, got:\n{contents}"
        );
        assert!(
            contents.contains("bridge |"),
            "new drained lines must be appended, got:\n{contents}"
        );
    }

    #[tokio::test]
    /// NF-R-042 — `--log-file` expands a leading `~` to the home directory.
    async fn ut_bridge_log_file_expands_tilde() {
        let home = std::env::home_dir().expect("HOME must resolve in test environment");
        let filename = format!("ferrowl_cl_bridge_tilde_{}.log", std::process::id());
        let expected_path = home.join(&filename);
        let _ = std::fs::remove_file(&expected_path);

        let downstream_port = free_port();
        let (_downstream_tx, _downstream) = seeded_downstream_server(downstream_port, 5).await;
        let upstream_port = free_port();
        let args = BridgeArgs {
            upstream: Some(descriptor(upstream_port)),
            downstream: Some(descriptor(downstream_port)),
            duration: Some(1),
            log_file: Some(format!("~/{filename}")),
            exit_on_error: false,
        };
        assert_eq!(run(&args).await, 0);

        let contents = std::fs::read_to_string(&expected_path);
        let _ = std::fs::remove_file(&expected_path);
        assert!(
            contents.unwrap().contains("bridge |"),
            "expected the log to have been written under the expanded home path"
        );
    }
}
