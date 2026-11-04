//! Binary-level register-event-script tests: a trigger the module cannot take fails the load.

use ferrowl_test_support::{reserve_tcp_port, reserve_temp_dir};
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferrowl"))
}

#[test]
/// SC-E-050 — an interval trigger on a Modbus device script exits 1, naming the line and
/// the key path of the trigger.
fn it_interval_trigger_fails_the_load_with_line_and_key_path() {
    let dir = reserve_temp_dir("ferrowl_it_interval_trigger");
    let device = dir.join("device.toml");
    std::fs::write(
        &device,
        "[definitions.reg]\ntype = \"U16\"\n\n[[scripts]]\nname = \"s\"\n[scripts.trigger]\nkind = \"interval\"\n",
    )
    .unwrap();
    let port = reserve_tcp_port().release();
    let module = format!(
        "name=it-trigger,device={},transport=tcp,ip=127.0.0.1,port={port},role=server",
        device.display()
    );
    let output = bin()
        .args(["run", "--module", &module, "--duration", "1"])
        .output()
        .expect("failed to run ferrowl binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("scripts[0].trigger"), "{stderr}");
    assert!(stderr.contains("line 7"), "{stderr}");
}

#[test]
/// SC-E-050 — a trigger on a session script exits 1, naming the line and the key path of
/// the trigger.
fn it_session_trigger_fails_the_load_with_line_and_key_path() {
    let dir = reserve_temp_dir("ferrowl_it_session_trigger");
    let session = dir.join("session.toml");
    std::fs::write(
        &session,
        "[[scripts]]\nname = \"s\"\n[scripts.trigger]\nkind = \"register\"\nregisters = \"all\"\n",
    )
    .unwrap();
    let output = bin()
        .args([
            "run",
            "--session",
            session.to_str().unwrap(),
            "--duration",
            "1",
        ])
        .output()
        .expect("failed to run ferrowl binary");

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("scripts[0].trigger"), "{stderr}");
    assert!(stderr.contains("line 4"), "{stderr}");
}
