# Ferrowl — Product Requirements

Why ferrowl exists, who it is for, and what is deliberately out of scope. This is
the product framing; the normative, testable behavior lives per capability under
[`docs/specs/`](./docs/specs/), and the structural map is
[`ARCHITECTURE.md`](./ARCHITECTURE.md). This file links to those rather than
duplicating them.

## Overview

Ferrowl is a terminal (TUI) application, written in Rust, that simulates
industrial and EV-charging network devices for testing and integration work:

- **Modbus** — client and server, over TCP and RTU (serial).
- **OCPP** (Open Charge Point Protocol) — Charging Station (client) and Central
  System / CSMS (server), versions 1.6, 2.0.1, and 2.1, over JSON-on-WebSocket.

Multiple independent module instances — any mix of Modbus/OCPP, client/server, any
version — run side by side as tabs in one process, each with its own live
register/state table, message log, and optional Lua-scripted behavior.
Configurations and whole multi-module sessions save to and load from TOML/JSON
files, so a test rig can be reproduced exactly on demand.

It targets engineers who need a protocol-accurate stand-in for real hardware — an
EVSE, a meter, a CSMS — without a GUI: CI runners, SSH sessions, headless test
rigs.

## Goals

- Simulate Modbus servers/clients and OCPP charging stations/CSMSes with enough
  protocol fidelity to stand in for real devices in integration testing.
- Make register/state manipulation and inspection fast and visual, even in a
  terminal.
- Let simulated device *behavior* — not just static values — be scripted, so a
  session can model something dynamic: a charging session ramping power, a meter
  drifting, a CS responding to remote commands.
- Make setups reproducible: everything material to a test rig's behavior is
  expressible in a config file, diffable and version-controllable.
- Run the same simulation logic interactively (TUI) or headlessly (`ferrowl run`,
  for CI).

## Non-goals

- **Not a GUI application** — no plans for one; GUI-preferring users are pointed
  elsewhere (e.g. QModbus).
- **Not a conformance/certification suite** — it simulates a device, it does not
  validate a real one against the spec.
- **Not a general-purpose scripting platform** — the Lua surface is deliberately
  narrow (register/OCPP-state access, logging, time) and runs in a restricted
  sandbox with no filesystem, shell, or dynamic-code access.
- **Not a persistence/database layer** — all simulated state is in-memory for the
  life of the process; only configuration, never live register/transaction state,
  survives a restart.

## Users & use cases

Primary user: an engineer integrating or testing software that talks Modbus or
OCPP — building a charge-point management system and needing a fake Charging
Station to drive it, or writing a SCADA integration and needing a fake Modbus
meter.

1. **EVSE simulation** — bring up a fake Charging Station (1.6/2.0.1/2.1) that
   connects to a real or test CSMS, and drive it through
   boot/authorize/start-transaction/meter-values/stop-transaction via Lua or
   manual action dialogs.
2. **CSMS simulation** — bring up a fake Central System that accepts real charge
   points, to test a CS implementation without a live backend.
3. **Modbus device simulation** — stand in for a meter/PLC/inverter as a Modbus
   TCP or RTU server, with registers seeded from a device profile and optionally
   ticking via Lua.
4. **Modbus polling client** — poll a real or simulated Modbus server, to inspect
   and exercise it interactively.
5. **CI-driven scenario runs** — `ferrowl run --session … --duration 60
   --exit-on-error`, headless, so a session's Lua scripts can assert expected
   behavior (`C_Test:Assert`) and fail the build on error.
6. **Config migration** — `ferrowl migrate` brings a pre-v0.4.0 (`modbus-cli-rs`)
   config forward to the current format.

## Feature surface

Summarized here; each links to the exhaustive, normative spec.

| Capability | Summary | Spec |
|---|---|---|
| Modbus | Client & server, TCP & RTU; all four register tables; typed registers (13 formats, both endians, bit-fields, display scaling); client auto-reconnect with backoff. | [`docs/specs/modbus/`](./docs/specs/modbus/) |
| OCPP | CS & CSMS roles; 1.6 (28 actions), 2.0.1 (64), 2.1 (90); TLS + mutual TLS + Basic Auth; typed send dialogs where the payload is flat, raw-JSON editor where it is nested. | [`docs/specs/ocpp/`](./docs/specs/ocpp/) |
| Scripting | Lua 5.4 sim scripts on a timer, per module and per session, via a small fixed `C_*` API, in a restricted sandbox. | [`docs/specs/scripting/`](./docs/specs/scripting/) |
| TUI | Tabbed multi-module UI, live tables, modal dialogs, a vim-style `:` command line, vim + arrow navigation, an in-TUI code editor. | [`docs/specs/tui/`](./docs/specs/tui/) |
| Config & session | Device + session files, TOML or JSON, save/load, `migrate`. | [`docs/specs/config-session/`](./docs/specs/config-session/) |
| CLI & headless | The process command line and `ferrowl run` for CI, with an exit-code contract. | [`docs/specs/cli-headless/`](./docs/specs/cli-headless/) |

Cross-cutting non-functional requirements (platforms, performance posture,
security posture, versioning) are specified in
[`docs/specs/non-functional-requirements.md`](./docs/specs/non-functional-requirements.md).
Each area's known limitations live in its own `edge-cases.md`.

## Glossary

| Term | Meaning |
|---|---|
| CS | Charging Station — the OCPP client role (the physical/simulated charge point). |
| CSMS | Charging Station Management System — the OCPP server role (Central System in 1.6). |
| EVSE | Electric Vehicle Supply Equipment — a charging connector/outlet, addressable in OCPP 2.0.1/2.1. |
| Coil / Discrete Input / Holding Register / Input Register | The four Modbus register spaces (1-bit R/W, 1-bit RO, 16-bit R/W, 16-bit RO). |
| Slave id / Unit id | Modbus device address on a shared bus/connection. |
| RTU | Modbus over serial (as opposed to TCP). |
| Module | One configured Modbus or OCPP instance, shown as one tab. |
| Session | A saved file listing multiple module instances plus session-level Lua scripts, restorable in one command. |
| Sim / sim script | A Lua script attached to a module (or the session) that runs on a timer to simulate device behavior. |
| Virtual register | A Modbus register with no fixed wire address — script-only, never sent over the wire. |
| `C_*` modules | The fixed set of Lua global tables (`C_Register`, `C_OCPP`, `C_Time`, `C_Log`, `C_Test`, `C_Module`, `C_Statics`) exposed to sim scripts. |
| Headless mode | `ferrowl run` — runs modules and Lua sims without a terminal UI, for CI. |
