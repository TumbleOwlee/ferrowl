//! Integration smoke tests for `ferrowl run` (headless/CI mode). Drives the actual compiled
//! binary as a subprocess since `ferrowl` is bin-only (no lib target to call `headless::run`
//! from directly), asserting the exit-code contract documented in the README.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferrowl"))
}

#[test]
/// CL-R-032 — a headless run reaching its --duration deadline exits 0.
fn it_runs_a_modbus_server_and_exits_clean() {
    let device = concat!(env!("CARGO_MANIFEST_DIR"), "/../configs/evse.toml");
    let module = format!(
        "name=it-headless-1,device={device},transport=tcp,ip=127.0.0.1,port=15920,role=server"
    );
    let output = bin()
        .args(["run", "--module", &module, "--duration", "1"])
        .output()
        .expect("failed to run ferrowl binary");

    assert!(
        output.status.success(),
        "expected exit 0, got {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("it-headless-1"),
        "expected drained log line naming the module, got: {stdout}"
    );
}

#[test]
/// CL-R-030 — a module whose device config fails to load makes the headless run exit 1.
fn it_fails_hard_on_a_missing_device_config() {
    let module = "name=it-headless-bad,device=/no/such/device.toml,transport=tcp,ip=127.0.0.1,port=15921,role=server";
    let output = bin()
        .args(["run", "--module", module, "--duration", "1"])
        .output()
        .expect("failed to run ferrowl binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to load"),
        "expected a load-failure message, got: {stderr}"
    );
}
