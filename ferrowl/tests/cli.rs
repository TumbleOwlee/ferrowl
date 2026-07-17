//! Subprocess tests for the ferrowl **process** CLI: SIGINT shutdown, `migrate` exit codes, and
//! the stderr-diagnostics contract. These drive the compiled binary because the behaviors are
//! genuinely process-level (async signal handling, `std::process::exit`, stdout/stderr
//! separation) and cannot be observed by calling a library function.

use std::process::{Command, Stdio};
use std::time::Duration;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferrowl"))
}

fn evse_device() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../configs/evse.toml")
}

#[test]
/// CL-R-025 — a SIGINT (Ctrl-C) during a headless run ends the loop as a clean shutdown (exit 0).
fn it_sigint_is_a_clean_shutdown() {
    let module = format!(
        "name=sig,device={},transport=tcp,ip=127.0.0.1,port=15931,role=server",
        evse_device()
    );
    // No --duration: the run loops until interrupted.
    let mut child = bin()
        .args(["run", "--module", &module])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ferrowl run");

    // Let it build the module and enter the tick loop before signalling.
    std::thread::sleep(Duration::from_millis(800));
    assert!(
        Command::new("kill")
            .args(["-INT", &child.id().to_string()])
            .status()
            .expect("send SIGINT")
            .success()
    );

    let status = child.wait().expect("wait for ferrowl");
    assert_eq!(status.code(), Some(0), "SIGINT must be a clean exit 0");
}

#[test]
/// CL-R-033 — migrate exits 1 on failure (unrecognized extension) and 0 on success, never 2.
/// CL-R-012 — migrate is dispatched directly, exiting with its own code without starting the TUI
/// or a headless run.
fn it_migrate_exit_codes() {
    let dir = std::env::temp_dir();

    // Failure: an input path whose extension is neither .toml nor .json.
    let bad = bin()
        .args([
            "migrate",
            "-i",
            "/tmp/ferrowl_migrate_input.bin",
            "-o",
            dir.join("ferrowl_migrate_bad_out.toml").to_str().unwrap(),
        ])
        .output()
        .expect("run migrate");
    assert_eq!(bad.status.code(), Some(1), "unknown extension must exit 1");
    assert!(
        String::from_utf8_lossy(&bad.stderr)
            .to_lowercase()
            .contains("error"),
        "a diagnostic beginning `error:` must be written to stderr"
    );

    // Success: a default (empty) legacy config converts and is written out.
    let input = dir.join("ferrowl_migrate_input.toml");
    std::fs::write(&input, "").unwrap();
    let output = dir.join("ferrowl_migrate_out.toml");
    let ok = bin()
        .args([
            "migrate",
            "-i",
            input.to_str().unwrap(),
            "-o",
            output.to_str().unwrap(),
        ])
        .output()
        .expect("run migrate");
    assert_eq!(
        ok.status.code(),
        Some(0),
        "a successful migration must exit 0; stderr: {}",
        String::from_utf8_lossy(&ok.stderr)
    );
}

#[test]
/// CL-R-042 — setup/fatal diagnostics go to stderr, keeping stdout as the drained-log stream.
fn it_fatal_diagnostics_go_to_stderr() {
    let module =
        "name=x,device=/no/such/device.toml,transport=tcp,ip=127.0.0.1,port=15932,role=server";
    let out = bin()
        .args(["run", "--module", module, "--duration", "1"])
        .output()
        .expect("run ferrowl");
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("Error:"),
        "the fatal diagnostic must be on stderr"
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("Error:"),
        "stdout must stay the machine-readable log stream, free of the diagnostic"
    );
}
