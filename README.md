![Banner](./images/ferrowl-banner.svg)

# Ferrowl - TUI Modbus + OCPP Server/Client

[![Claude](https://img.shields.io/badge/Claude-D97757?logo=claude&logoColor=fff)](#) [![status-badge](https://github-ci.code-ape.dev/api/badges/3/status.svg?workflow=check)](https://github-ci.code-ape.dev/repos/3) [![status-badge](https://github-ci.code-ape.dev/api/badges/3/status.svg?workflow=nightly)](https://github-ci.code-ape.dev/repos/3)

Ferrowl is a TUI application, written in Rust, to simulate both **Modbus** (Client and Server) and **OCPP** (Charging Station / CSMS) devices. Create configurations on the fly, save and load configurations or sessions to set up multiple Modbus or OCPP instances side by side. The aim is to provide a technical but intuitive interface that can run on any device without an available GUI environment.

If you prefer a GUI application, this tool is not the right choice. For Modbus, refer to a GUI application like [QModbus](https://github.com/ed-chemnitz/qmodbus/).

> [!WARNING]
> Prior to **v0.4.0** the application was based on a draft implementation. Over time additional features were added but messed up the architecture and made it difficult to add new views, dialogs and support of multiple instances. Thus, starting with **v0.4.0** the application got a full rewrite. This also affects the configuration files and their management. You can migrate configuration files created by versions prior to **v0.4.0** using the `migrate` subcommand - e.g. `ferrowl migrate -i old-config.json -o new-config.json` (supports JSON and TOML as input and output).

## Goal

Provide a CLI application to simulate Modbus Servers and Clients as well as OCPP Charging Stations and Central Systems (CSMS, OCPP 1.6, 2.0.1 and 2.1), visualize the states of all registers and charging-station fields, make manipulation available and provide script based simulation capabilities - e.g. utilize the tool to simulate EVSEs over Modbus or OCPP.

## Architecture

The project is organized as a Cargo workspace and builds the `ferrowl` binary. See the following image for the dependencies between the different crates of the workspace and their provided functionality.

<p align="center">
    <img src="./images/architecture.svg">
</p>


| Crate | Responsibility |
| ----- | ----- |
| `ferrowl` | Binary. Event/redraw loop, tabs, views, dialogs, `:` commands, session & device configuration, `migrate` subcommand. |
| `ferrowl-ui` | Reusable [ratatui](https://ratatui.rs) building blocks: widgets with their state types, styling and alternate-screen handling. |
| `ferrowl-ui-derive` | Proc macros for the UI layer: `#[derive(TableEntry)]`, `#[derive(Overlay)]` and `#[derive(Focus)]` (keyboard focus cycling and event dispatch for views). |
| `ferrowl-lua-derive` | Proc macro `#[derive(Module)]` that bridges Rust host types into Lua modules. |
| `ferrowl-codec` | Register descriptions (slave id, function code, address, access, format) and the codec between raw `u16` words and typed values. |
| `ferrowl-store` | In-memory model of a Modbus register space — access-checked value cells shared as `Arc<RwLock<Memory>>`. |
| `ferrowl-modbus` | Modbus client and server tasks over TCP and RTU, built on [tokio-modbus](https://github.com/slowtec/tokio-modbus). |
| `ferrowl-ocpp` | OCPP protocol types and actions with a version-generic `Version` trait; Charging Station (CS) and Central System (CSMS) over JSON-on-WebSocket, wrapping [rust-ocpp](https://github.com/codelabsab/rust-ocpp). Supports OCPP 1.6, 2.0.1, and 2.1. |
| `ferrowl-lua` | Embedded Lua runtime ([mlua](https://github.com/mlua-rs/mlua)) exposing the `C_Register`, `C_Time`, `C_OCPP` and `C_Log` modules to simulation scripts. |
| `ferrowl-ring` | Fixed-capacity ring buffer generic over the element type; backs the per-module log pane (as `Ring<(u64, String), N>`). |
| `ferrowl-util` | Shared helpers: config (de)serialization, tracked tokio task spawning, small macros and traits. |

All runtime interaction meets in the shared memory of a module: the network task polls a remote server (client role) or answers incoming requests (server role) against it, the Lua simulation thread reads and writes it through the `C_Register` bridge, and the UI decodes its raw words into the typed values shown in the register table.

## OCPP

Alongside Modbus, Ferrowl simulates **OCPP** charging infrastructure over JSON-on-WebSocket. Both protocol versions and both roles are supported:

- **Versions:** OCPP **1.6**, **2.0.1**, and **2.1**.
- **Roles:** **Charging Station** (client, connects out to a CSMS) and **Central System / CSMS** (server, accepts incoming stations and tracks each connection).

Supported capabilities (grouped by area):

- **Transactions & metering** — start/stop transactions, `MeterValues`, live connector state (status, phases, voltage, per-phase current, power, energy).
- **Reservations** — `ReserveNow` / `CancelReservation`, per connector.
- **Authorization** — RFID accept-lists, both station-wide and per-connector, plus local-list management.
- **Smart charging** — charging profiles and per-purpose charge limits, including stack-level reject.
- **Remote control** — remote (1.6) / requested (2.0.1, 2.1) start & stop, availability changes, reset, firmware update and diagnostics.
- **OCPP 2.0.1 extras** — variable get/set and monitoring, display messages, certificate management, and the EVSE/connector object model.

In the TUI each OCPP module shows a connector/station table, a scope-filtered action list with per-version send dialogs (typed value editors plus a raw-JSON mode), and a capped message log that can be mirrored to a file via `:log`. Simulation behaviour is scripted in Lua for both roles — see [Lua Support](#lua-support).

## Nightly Build

This repository provides an updated Nightly build - available on the Release page. Prebuilt executables are provided for Unix and Windows.

## Quickstart

This project is written in Rust, thus you will have to install the Rust toolchain to compile it. Just follow the instructions on [rustup.rs](https://rustup.rs/) to set up the environment. Afterwards you are able to compile this project from source using the following command.

```sh
cargo build --release
```

Alternatively, you can also run it directly using the following command. Please refer to `--help` for all available runtime options and to the Release page for prebuilt binaries.

```bash
# Build and run
cargo run --release

# Or with the application already built
ferrowl

# Or in demo mode
ferrowl --demo

# Or with an existing session file
ferrowl --session session.toml

# Or with a device configuration only (starts a TCP client polling 127.0.0.1:5020,
# matching the --demo server; use --module for a custom endpoint or role)
ferrowl --device device.toml
```

If started without any additional parameters, the module setup dialog is shown. After the module is created, you can add registers using the `:add` command. To create an OCPP module instead, choose the OCPP type in the `:new` setup dialog.

The bundled `session.toml` wires up a CSMS plus a Charging Station pair (`csms-demo.toml` / `cs-demo.toml`), so `ferrowl --session session.toml` brings both OCPP modules online at once.

> [!IMPORTANT]
> You can use *VIM*-like table navigation or alternatively the arrow keys. You can exit using the `:qa` command. Typing `:` will automatically switch to command mode. See the shown overlay for all available commands.

## Commands

| Command | Description |
| ----- | ----- |
| `:q \| :quit` | Quit tab / Close active module |
| `:qa \| :qall` | Close all tabs / Exit application |
| `:e \| :edit` | Edit current module |
| `:n \| :new` | Create new module |
| `:l \| :load [PATH]` | Load device configuration |
| `:a \| :add` | Add new register to module |
| `:start` | Start module execution |
| `:stop` | Stop module execution |
| `:restart` | Restart module execution |
| `:set <reg> <val>` | Write register value |
| `:s \| :save \| :w \| :write [PATH]` | Save session |
| `:wd \| :write-device [PATH]` | Save device configuration |
| `:log <FILE>\|clear` | Set log output file for the active tab (`:log clear` clears the ring log) |
| `:lua start\|stop` | Start/Stop lua execution |
| `:reload` | Reload device configuration |
| `:compact` | Toggle compact table mode |
| `:order [col] [asc\|desc]` | Sort table by column |

## Keybindings

| Keybind | Description |
| ----- | ----- |
| `Enter` | Open/Confirm dialog |
| `Escape` | Cancel dialogs |
| `k \| Up` | Select previous table entry |
| `h \| Left` | Scroll left in table view |
| `l \| Right` | Scroll right in table view |
| `j \| Down` | Select next table entry |
| `Tab` | Focus next dialog element |
| `Shift-Tab` | Focus previous dialog element |
| `Space` | Click focused button |
| `G` | Move to bottom of table |
| `g` | Move to top of table |
| `0` | Move to left edge of table |
| `$` | Move to right edge of table |
| `z` | Toggle compact table mode |
| `gt \| ]` | Switch to next tab |
| `gT \| [` | Switch to previous tab |
| `ctrl + d` | Clear input field |
| `ctrl + f` | Accept autofill in input field |

## Impressions

### General

#### Command Help

<p align="center">
    <p align="center">
        <img src="./images/command-help.png" style="border-radius: 8px">
    </p>
</p>

#### Dialog: New Module

<p align="center">
    <p align="center">
        <img src="./images/new-module.png" style="border-radius: 8px">
    </p>
</p>

### Modbus

#### Dialog: New Module

<p align="center">
    <p align="center">
        <img src="./images/modbus/new-module.png" style="border-radius: 8px">
    </p>
</p>

#### Dialog: Add Register

<p align="center">
    <p align="center">
        <img src="./images/modbus/add-register.png" style="border-radius: 8px">
    </p>
</p>

#### Dialog: Edit Register

<p align="center">
    <p align="center">
        <img src="./images/modbus/edit-register.png" style="border-radius: 8px">
    </p>
</p>

#### Dialog: Edit Selection Register

<p align="center">
    <p align="center">
        <img src="./images/modbus/edit-selection-register.png" style="border-radius: 8px">
    </p>
</p>

### OCPP 

#### Dialog: New Module

<p align="center">
    <p align="center">
        <img src="./images/ocpp/new-module.png" style="border-radius: 8px">
    </p>
</p>

#### Dialog: Action

<p align="center">
    <p align="center">
        <img src="./images/ocpp/action.png" style="border-radius: 8px">
    </p>
</p>

#### Client View - General

<p align="center">
    <p align="center">
        <img src="./images/ocpp/client.png" style="border-radius: 8px">
    </p>
</p>

#### Client View - CS

<p align="center">
    <p align="center">
        <img src="./images/ocpp/client-cp.png" style="border-radius: 8px">
    </p>
</p>

#### Client View - Connector

<p align="center">
    <p align="center">
        <img src="./images/ocpp/client-con.png" style="border-radius: 8px">
    </p>
</p>

#### Server View - General

<p align="center">
    <p align="center">
        <img src="./images/ocpp/server.png" style="border-radius: 8px">
    </p>
</p>

#### Server View - CS

<p align="center">
    <p align="center">
        <img src="./images/ocpp/server-cp.png" style="border-radius: 8px">
    </p>
</p>

#### Server View - Connector

<p align="center">
    <p align="center">
        <img src="./images/ocpp/server-con.png" style="border-radius: 8px">
    </p>
</p>

## Configuration

### Session Configuration

The session configuration can be saved using `:write` and contains the module configuration consisting of the name, path to the device configuration, the role and endpoint information. Timings (`timeout_ms`, `delay_ms`, `interval_ms`) are part of the device configuration, not the session.

```toml
[[modules]]
name = "evse-1"
device = "configs/evse.toml"
role = "server"

[modules.endpoint]
transport = "tcp"
ip = "127.0.0.1"
port = 5020
```

Besides TCP, a serial RTU endpoint is supported. `parity` (`even`, `odd` or `none`, case-insensitive), `data_bits` and `stop_bits` are optional; `baud_rate` defaults to `19200`.

```toml
[modules.endpoint]
transport = "rtu"
path = "/dev/ttyUSB0"
baud_rate = 19200
parity = "none"
data_bits = 8
stop_bits = 1
```

An **OCPP** module session entry is tagged `type = "ocpp"` and carries only the name, the
device-config path and the websocket endpoint (`protocol` is `ws` or `wss`); the OCPP version,
role, timeout and Lua scripts live in the referenced device file.

```toml
[[modules]]
type = "ocpp"
name = "cs-1"
device = "configs/cs.toml"
protocol = "ws"
ip = "127.0.0.1"
port = 9000
```

### Device Configuration

The device configuration can be saved using `:write-device` and contains the register information of the device and all necessary timings.

```toml
[definitions.setpoint]
slave_id = 1
read_code = 4          # 4 = holding register
address = 0
type = "U16"
access = "ReadWrite"
description = "charge setpoint (W)"
default = 0            # start at zero watts on every load

[definitions.power]
slave_id = 1
read_code = 4
address = 1
type = "U16"
access = "ReadWrite"
description = "active power (W)"
default = 0
# Lua run every cycle: mirror the setpoint into the power register (server simulation).
update = """
C_Register:Set("power", C_Register:Get("setpoint"))
"""

[definitions.state]
slave_id = 1
read_code = 4
address = 2
type = "I16"
access = "ReadWrite"
description = "charge state"
default = 0            # start in the "waiting" state
values = [
    { name = "waiting", value = 0 },
    { name = "charging", value = 2 },
    { name = "error", value = -1 },
]
```

> [!IMPORTANT]
> It's possible to create virtual registers using `virtual = true` to store values not accessible over Modbus.

#### Register fields

| Field | Default | Description |
| ----- | ----- | ----- |
| `type` | *(required)* | Value encoding: `U8`…`U128`, `I8`…`I128`, `F32`, `F64`, `Ascii`. |
| `slave_id` | `0` | Modbus unit / slave id. |
| `read_code` | `3` | Read function code: `1`=Coil, `2`=DiscreteInput, `3`=InputRegister, `4`=HoldingRegister. |
| `address` | *(none)* | Start address. Omit (or set `virtual = true`) for a virtual register. |
| `virtual` | `false` | Hold the value locally instead of mapping it to a Modbus address. |
| `access` | `ReadWrite` | `ReadOnly`, `WriteOnly` or `ReadWrite`. |
| `endian` | `Big` | Byte order: `Big` or `Little`. |
| `resolution` | `1.0` | Scaling factor applied to the raw value for display; edit dialogs and `:set` take the unscaled raw value. |
| `bitmask` | *(none)* | Bit-field mask for integer types, as a hex (`"0xFF00"`) or decimal string; the shift is derived from the mask's trailing zeros. Ignored for float and ASCII types. |
| `length` | `1` | ASCII width in registers (ignored for numeric types). |
| `alignment` | `Left` | ASCII alignment: `Left` or `Right`. |
| `values` | `[]` | Named values for selection-style registers. |
| `default` | *(none)* | Default value written to memory on startup / configuration load. |
| `update` | *(none)* | Lua snippet run every simulation cycle. |
| `description` | `""` | Free-text description. |

#### Device-level options

Alongside `definitions`, a device file may carry a `version` (stamped automatically on save), default timings, and explicit client read ranges:

```toml
description = "EVSE charge point"
timeout_ms = 2000      # per-request timeout
delay_ms = 1000        # delay before the first read
interval_ms = 1000     # poll interval

# Client read batching per function code. Each value is a comma-separated list of inclusive
# address ranges (a bare "5" is the single address 5). When unset, contiguous registers are
# auto-merged into requests.
[read_ranges]
holding = "0-100,140-160"
input = "0-10"
# coils / discrete are also available
```

Timing precedence is device → built-in defaults (3000/1000/1000 ms).

An **OCPP** device file (saved with `:write-device`) describes the charge point: its OCPP version,
role, reply timeout and the Lua simulation scripts. Endpoint (ip/port/protocol) is per-instance and
lives in the session, not here.

```toml
version = "0.4.4"        # ferrowl version, stamped on save
ocpp_version = "1.6"     # or "2.0.1" or "2.1"
role = "client"          # client = charging station, server = management system
timeout_ms = 30000       # awaited-reply timeout

[[scripts]]
name = "ramp"
enabled = true
code = "C_OCPP:Set(\"Power\", C_OCPP:Get(\"Power\") + 100)"
```

## Lua Support

As an additional feature, the tool also includes a Lua runtime to execute custom scripts that drive a simulation. For **Modbus** modules these are the per-register `update` snippets (run each cycle), interacting with the registers through `C_Register` and able to print to the module log via `C_Log`. For **OCPP** modules — in both the Charging Station (client) and CSMS (server) roles — scripts are attached to the device config and managed from the *Lua Scripts* dialog (the button under the state table); all enabled scripts run about once per second and interact with the OCPP state and actions through `C_OCPP`, and may print to the module log via `C_Log`. Besides the standard Lua libraries, the exposed modules are `C_Time` and `C_Log` (both), `C_Register` (Modbus only), and `C_OCPP` (OCPP only).

### Module C_Time

```
Method:   C_Time:Get()

Arguments: None

Return: Time in seconds since startup.
```
```
Method:   C_Time:GetMs()

Arguments: None

Return: Time in milliseconds since startup.
```

### Module C_Log

```
Method:   C_Log:Print(message)

Arguments:
               Name: message
               Type: String
        Description: A line to append to the module's log (the on-screen log
                     pane, and the file sink when `:log <file>` is active).

Return: nil
```

### Module C_Register

```
Method:   C_Register:Get(name)

Arguments:
               Name: name
               Type: String
        Description: Name of the register as defined in the configuration.

Return: Value of the register, typed to match it: a number for integer and
        floating-point registers, a string for strings and a boolean for booleans.
```
```
Method:   C_Register:Set(name, value)

Arguments:
               Name: name
               Type: String
        Description: Name of the register as defined in the configuration.

               Name: value
               Type: String | bool | integer | float
        Description: Value to set for the specified register

Return: nil
```

### Module C_OCPP

Exposed to the Lua scripts of an **OCPP** module, in both the charging-station (client) and CSMS
(server) roles. All loaded, enabled scripts run about once per second. `C_Time` is also available;
`C_Register` is **not** (it is Modbus-only).

The module has a flat surface plus role-specific scope accessors:

- **Client** — bare `Get`/`Set`/`<Action>` address the charging station itself; `Connector(id)`
  returns an accessor scoped to one connector with the same `Get`/`Set`/`<Action>` surface.
- **Server (CSMS)** — `GetChargingStations()` and `GetConnectors(cs)` enumerate the connected
  stations and their connectors; `ChargingStation(cs)` and `Connector(cs, id)` return accessors
  scoped to one station or one of its connectors.

`Get`/`Set` read and write the addressed scope's state by name. Supported names (compact forms of
the state-table labels):

```
ConnectorId, Phases, Voltage, Current (= CurrentL1), CurrentL1, CurrentL2, CurrentL3,
Power, TotalEnergy, SessionEnergy, Status, Rfid, Model, Vendor
```

OCPP 2.0.1 additionally exposes `EvseId`.

```
Method:   C_OCPP:Get(name)

Arguments:
               Name: name
               Type: String
        Description: State field name (see the list above).

Return: Value of the field — a number for numeric fields, a string for textual ones — or an
        error for an unknown name.
```
```
Method:   C_OCPP:Set(name, value)

Arguments:
               Name: name
               Type: String
        Description: State field name. Numeric fields accept an integer or float; textual
                     fields accept a string.

               Name: value
               Type: integer | float | string

Return: nil (errors on an unknown name or a type mismatch).
```

In addition, every supported OCPP action is callable as `C_OCPP:<Action>(overrides?)`. The set of
actions is version-specific (OCPP 1.6, 2.0.1 and 2.1 differ), so a script must match the device's OCPP
version. The action's payload is built from the current state exactly like the on-screen action
buttons; an optional table of overrides is shallow-merged over it. The call returns `true` once the
action is queued, or `false` on an argument error. The result of the exchange with the CSMS appears
in the module's Messages table, not in the return value.

```
Method:   C_OCPP:<Action>(overrides?)
          e.g. C_OCPP:Authorize(), C_OCPP:StartTransaction(), C_OCPP:MeterValues()
               C_OCPP:BootNotification({ chargePointModel = "Custom" })

Arguments:
               Name: overrides
               Type: table (optional)
        Description: Key/value fields shallow-merged over the state-derived payload.

Return: true when the action was queued, false on an argument error.
```

#### Scope accessors

The same `Get`/`Set`/`<Action>` surface is reachable on a narrower scope. On the **client** a
connector accessor is obtained by id; on the **server** the connected stations and their
connectors are enumerated and then addressed by id.

```
Method:   C_OCPP:Connector(id)                 -- client role
Return:   accessor scoped to connector `id`, exposing Get/Set/<Action>.

Method:   C_OCPP:GetChargingStations()         -- server role
Return:   list of connected charging-station ids.

Method:   C_OCPP:GetConnectors(cs)             -- server role
Return:   list of connector ids seen for station `cs`.

Method:   C_OCPP:ChargingStation(cs)           -- server role
Return:   accessor scoped to station `cs` (or nil if unknown), exposing Get/Set/<Action>.

Method:   C_OCPP:Connector(cs, id)             -- server role
Return:   accessor scoped to connector `id` of station `cs` (or nil if unknown),
          exposing Get/Set/<Action>.
```

#### Example

```lua
-- Ramp the charging current while a transaction is running, then report it.
local target = 16.0
local current = C_OCPP:Get("CurrentL1")
if current < target then
    C_OCPP:Set("CurrentL1", current + 0.5)
    C_OCPP:Set("CurrentL2", current + 0.5)
    C_OCPP:Set("CurrentL3", current + 0.5)
    C_OCPP:MeterValues()
end
```
