//! Binary-level register-event-script tests: a trigger kind the module cannot take fails the load,
//! and a remote write runs the script watching it. The Modbus frames are hand-built per the
//! Modbus Application Protocol specification (MBAP header, unit id, function code, big-endian
//! address and value).

use ferrowl_test_support::{TempDirGuard, reserve_tcp_port, reserve_temp_dir};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_ferrowl"))
}

#[test]
/// SC-E-050 — a trigger of an unknown kind on a Modbus device script exits 1, naming the
/// line and the key path of the trigger.
fn it_unknown_trigger_kind_fails_the_load_with_line_and_key_path() {
    let dir = reserve_temp_dir("ferrowl_it_unknown_trigger");
    let device = dir.join("device.toml");
    std::fs::write(
        &device,
        "[definitions.reg]\ntype = \"U16\"\n\n[[scripts]]\nname = \"s\"\n[scripts.trigger]\nkind = \"message\"\n",
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

struct Server {
    child: Child,
    port: u16,
    _dir: TempDirGuard,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Registers `names` as U16 holding registers (address = position, default 3) and `scripts`
/// (TOML tables for `[[scripts]]`), served on a free port.
fn serve(names: &[&str], scripts: &str) -> Server {
    let dir = reserve_temp_dir("ferrowl_it_register_events");
    let mut device = String::from("script_interval = 3600.0\n");
    for (address, name) in names.iter().enumerate() {
        device.push_str(&format!(
            "[definitions.{name}]\nslave_id = 1\nkind = \"HoldingRegister\"\naddress = {address}\ntype = \"U16\"\naccess = \"ReadWrite\"\ndefault = 3\n"
        ));
    }
    device.push_str(scripts);
    let path = dir.join("device.toml");
    std::fs::write(&path, device).expect("write device");
    let port = reserve_tcp_port().release();
    let module = format!(
        "name=it-events,device={},transport=tcp,ip=127.0.0.1,port={port},role=server",
        path.display()
    );
    let child = Command::new(env!("CARGO_BIN_EXE_ferrowl"))
        .args(["run", "--module", &module, "--duration", "60"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn ferrowl");
    let server = Server {
        child,
        port,
        _dir: dir,
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while try_request(port, 3, 0, 1).is_err() {
        assert!(Instant::now() < deadline, "server never answered");
        std::thread::sleep(Duration::from_millis(20));
    }
    server
}

fn try_request(port: u16, function: u8, address: u16, value: u16) -> std::io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut frame = vec![0, 1, 0, 0, 0, 6, 1, function];
    frame.extend_from_slice(&address.to_be_bytes());
    frame.extend_from_slice(&value.to_be_bytes());
    stream.write_all(&frame)?;
    let mut header = [0u8; 7];
    stream.read_exact(&mut header)?;
    let mut pdu = vec![0u8; usize::from(u16::from_be_bytes([header[4], header[5]])) - 1];
    stream.read_exact(&mut pdu)?;
    Ok(pdu)
}

fn request(port: u16, function: u8, address: u16, value: u16) -> Vec<u8> {
    try_request(port, function, address, value).expect("request")
}

fn write_register(port: u16, address: u16, value: u16) {
    let pdu = request(port, 6, address, value);
    assert_eq!(pdu[0], 6, "write rejected: {pdu:?}");
}

fn read_register(port: u16, address: u16) -> u16 {
    let pdu = request(port, 3, address, 1);
    assert_eq!(pdu[0], 3, "read rejected: {pdu:?}");
    u16::from_be_bytes([pdu[2], pdu[3]])
}

fn wait_register(port: u16, address: u16, want: u16) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while read_register(port, address) != want {
        assert!(
            Instant::now() < deadline,
            "register {address} never reached {want}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
/// SC-R-087 — a remote write runs the watching script, and the event carries the register name, old
/// and new value, source and sequence number.
fn it_remote_write_runs_watching_script() {
    let server = serve(
        &[
            "temp",
            "seen_old",
            "seen_new",
            "seen_seq",
            "name_ok",
            "source_ok",
        ],
        r#"
[[scripts]]
name = "watch"
code = '''
local e = ...
C_Register:Set("seen_old", e.old)
C_Register:Set("seen_new", e.new)
C_Register:Set("seen_seq", e.seq)
if e.name == "temp" then C_Register:Set("name_ok", 1) end
if e.source == "remote" then C_Register:Set("source_ok", 1) end
'''
[scripts.trigger]
kind = "register"
registers = ["temp"]
"#,
    );
    write_register(server.port, 0, 7);
    wait_register(server.port, 2, 7);
    wait_register(server.port, 3, 1);
    assert_eq!(read_register(server.port, 1), 3, "old");
    wait_register(server.port, 4, 1);
    wait_register(server.port, 5, 1);
}

#[test]
/// SC-R-092 — rewriting a register's current value runs only the every-write script.
fn it_same_value_runs_only_every_write_script() {
    let server = serve(
        &["temp", "changed", "every"],
        r#"
[[scripts]]
name = "on_change"
code = 'C_Register:Set("changed", C_Register:Get("changed") + 1)'
[scripts.trigger]
kind = "register"
registers = ["temp"]

[[scripts]]
name = "on_write"
code = 'C_Register:Set("every", C_Register:Get("every") + 1)'
[scripts.trigger]
kind = "register"
registers = ["temp"]
every_write = true
"#,
    );
    // The default is 3: writing 3 again changes nothing.
    write_register(server.port, 0, 3);
    wait_register(server.port, 2, 3 + 1);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(read_register(server.port, 1), 3, "changed moved");

    write_register(server.port, 0, 9);
    wait_register(server.port, 1, 3 + 1);
    wait_register(server.port, 2, 3 + 2);
}
