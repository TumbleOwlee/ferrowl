//! Integration smoke tests for `ferrowl bridge`. Drives the actual compiled binary as a
//! subprocess since `ferrowl` is bin-only (no lib target to call `cli::bridge::run` from
//! directly) — mirroring `tests/headless.rs`'s equivalent smoke tests for `ferrowl run`. The
//! richer end-to-end/log-file/exit-on-error scenarios live in-process instead, as `ut_`-
//! prefixed tests inside `ferrowl/src/cli/bridge.rs` itself (same split `headless.rs` already
//! uses between its own in-crate tests and this crate's `tests/headless.rs`).

// Integration-test crate: an unwrap that fails is the test failing, same as an assertion.
#![allow(clippy::unwrap_used)]

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferrowl"))
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
/// BR-R-001..BR-R-006, BR-R-013 — a bridge with no downstream listening still starts (BR-R-013's
/// setup-failure list does not include a downstream connect failure)
/// and, on its `--duration` deadline, exits 0.
fn it_bridge_starts_with_unreachable_downstream_and_exits_clean_on_deadline() {
    let upstream_port = free_port();
    let downstream_port = free_port();
    let output = bin()
        .args([
            "bridge",
            "--upstream",
            &format!("transport=tcp,ip=127.0.0.1,port={upstream_port}"),
            "--downstream",
            &format!("transport=tcp,ip=127.0.0.1,port={downstream_port}"),
            "--duration",
            "1",
        ])
        .output()
        .expect("failed to run ferrowl binary");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
/// BR-R-003, BR-R-013 — missing `--downstream` makes the bridge exit 1 without starting.
fn it_bridge_fails_hard_on_a_missing_downstream_flag() {
    let upstream_port = free_port();
    let output = bin()
        .args([
            "bridge",
            "--upstream",
            &format!("transport=tcp,ip=127.0.0.1,port={upstream_port}"),
            "--duration",
            "1",
        ])
        .output()
        .expect("failed to run ferrowl binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--upstream and --downstream"),
        "expected a missing-flag message, got: {stderr}"
    );
}
