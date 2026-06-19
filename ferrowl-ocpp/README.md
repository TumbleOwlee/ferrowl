# `ferrowl-ocpp`

OCPP charging-infrastructure simulation for `ferrowl`. It provides two cooperating roles over
WebSocket (OCPP-J):

- **CS** — *Charging Station* (client role; dials out to a CSMS). v1.6 called this a *Charge Point*.
- **CSMS** — *Charging Station Management System* (server role; accepts CS connections). v1.6
  called this a *Central System*.

Both **OCPP 1.6** (28 actions) and **OCPP 2.0.1** (64 actions) are supported simultaneously, gated
by the `v1_6` / `v2_0_1` Cargo features (both on by default).

It is built on [`rust-ocpp`](https://crates.io/crates/rust-ocpp) `3.0.4`, which supplies only the
typed per-action `Request`/`Response` structs (serde + `validator::Validate`). Everything else —
the OCPP-J envelope framing, the WebSocket transport, request/response correlation, and the
version-portable semantic API — lives in this crate.

This crate is the standalone abstraction layer: no UI, no Lua hooks, no `ferrowl` binary wiring
(those are consumed later, mirroring how `ferrowl-modbus` is used by `ferrowl/src/instance/mod.rs`).

## Architecture

### Two public API layers

1. **Low-level (full wire fidelity).** A [`Version`] trait per OCPP version, with generated
   `Action` / `Response` enums whose variants wrap `rust_ocpp`'s own structs untouched. You send and
   answer raw, typed actions via [`CsActionHandler`] / [`CsmsActionHandler`] and the `Command`
   channel. Nothing is hidden or lowest-common-denominatored.

2. **Semantic (version-portable).** One method per *logical* operation across
   [`CsOps`] / [`CsHandler`] (CS side) and [`CsmsOps`] / [`CsmsHandler`] (CSMS side), using this
   crate's own version-neutral parameter/result structs. You write simulation logic once and it runs
   unchanged against either OCPP version. Per-version adapters translate to/from the wire types;
   methods a version doesn't support default to `NotSupported`.

The semantic layer is built *on top of* the low-level layer — both are public, use whichever fits.

### Duplex connection engine

OCPP-J is bidirectional on a single socket (either peer may initiate a Call), so a connection is not
a request/response poll loop. Each connection runs three concurrent pieces (shared by CS and CSMS in
`conn.rs`):

- a **writer task** owning the WebSocket sink, fed every outbound frame over an mpsc channel;
- a **reader task** owning the stream that completes correlated replies and **spawns a handler task
  per inbound Call**, so a slow or re-entrant handler never blocks the read pump;
- a per-call **awaiter task** that owns the originating action and decodes the otherwise action-less
  `CallResult` payload into the typed response.

Correlation is a shared `PendingCalls` table (`UniqueId → oneshot`). The CSMS additionally runs an
accept loop that negotiates the WebSocket subprotocol against the server's version, parses the
charging-station identity from the URL path, assigns an opaque `ConnectionId`, and registers each
connection for command routing and broadcast.

### Module layout

```
ferrowl-ocpp/
  src/
    lib.rs
    error.rs          # Error / OcppError / FramingError / WsError / ValidationError / CallError
    log.rs            # LogFn (RPITIT async-callback idiom, no async-trait)
    correlation.rs    # PendingCalls correlation table
    conn.rs           # shared duplex engine: writer + reader + outbound handle
    ocppj/            # version-agnostic OCPP-J framing
      message.rs      #   MessageTypeId, UniqueId, OcppJMessage{Call,CallResult,CallError}
      codec.rs        #   OcppJMessage <-> WebSocket text frame (hand-written JSON arrays)
      error_code.rs   #   CallErrorCode (spec-fixed list)
    action/           # low-level action layer
      macros.rs       #   define_ocpp_version! declarative macro
      v1_6.rs         #   28-action table -> Action/Response enums + Version impl
      v2_0_1.rs       #   64-action table -> Action/Response enums + Version impl
      mod.rs          #   pub trait Version
    semantic/
      types.rs        # version-neutral Params/Result structs
    cs/               # Charging Station (client)
      command.rs core.rs action_handler.rs handler.rs ops.rs adapter.rs config.rs mod.rs
    csms/             # Charging Station Management System (server)
      command.rs core.rs action_handler.rs handler.rs ops.rs adapter.rs registry.rs config.rs mod.rs
  tests/
    ws_loopback_v16.rs        # low-level CS<->CSMS round trip over a real websocket
    ws_semantic_v16_v201.rs   # semantic round trip + wire-mapping checks, both versions
```

### Error model

Layered like `ferrowl-modbus`: `FramingError` (malformed envelope) → `WsError` (transport) →
`OcppError` (unknown action / (de)serialization / validation) → top-level [`Error`]. Separately,
[`CallError`] is what a handler *returns* to reject a Call at the protocol level — it is serialized
to a `CallError` frame and sent back over the wire, distinct from an [`Error`] that tears the
connection down. `validator::Validate` runs after decoding an inbound Call and before dispatch; a
failure short-circuits to a `FormationViolation` `CallError`.

## Quick start

### Low-level CS client

```rust
use ferrowl_ocpp::cs::{ClientBuilder, Config, CsActionHandler, Command};
use ferrowl_ocpp::{V1_6, Action16, Response16, CallError, CallErrorCode};

struct Handler;
impl CsActionHandler<V1_6> for Handler {
    async fn handle_call(&self, action: Action16) -> Result<Response16, CallError> {
        // answer CSMS-initiated calls; reject the rest at the protocol level
        Err(CallError::new(CallErrorCode::NotImplemented, "unsupported"))
    }
}

let client = ClientBuilder::<V1_6>::new(Config { url: "ws://host:9000/ocpp/CS001".into(), timeout_ms: 30_000 })
    .spawn(Handler, |_line| async {})
    .await?;

// Send a typed Call and await the typed reply.
let resp = client.call(Action16::Heartbeat(Default::default())).await?;
```

### Semantic CS client (version-portable)

```rust
use ferrowl_ocpp::cs::{ClientBuilder, Config, CsOps, SemanticAdapter};
use ferrowl_ocpp::types::{BootNotificationParams, StartTransactionParams};
use ferrowl_ocpp::V2_0_1;

// `MyCsHandler: CsHandler` answers CSMS-initiated calls with neutral types; wrap it per version.
let client = ClientBuilder::<V2_0_1>::new(cfg)
    .spawn(SemanticAdapter::<V2_0_1, _>::new(MyCsHandler::default()), |_l| async {})
    .await?;

// Same call works against v1.6 or v2.0.1; the adapter maps it to the right wire action.
let boot = client.boot_notification(BootNotificationParams { model: "M".into(), vendor: "Ferrowl".into() }).await?;
let tx   = client.start_transaction(StartTransactionParams { connector_id: 1, id_tag: "TAG1".into(), meter_start: 0, timestamp: "2026-01-01T00:00:00Z".into() }).await?;
```

The CSMS side mirrors this: `csms::ServerBuilder` + a `CsmsActionHandler` (low-level) or
`csms::SemanticAdapter::new(my_csms_handler)` (semantic), with `Server::call(conn, …)` and the
`CsmsOps` methods to drive a specific connected CS.

## Semantic method coverage

The low-level layer covers **every** action. The semantic layer wires **all both-version
actions** plus every cross-version merge and a representative v2.0.1-only method (`notify_event`),
on both OCPP versions. The version-only specialized actions (diagnostics, certificates, monitoring,
display messages, reporting, …) are not yet given semantic methods; use the low-level layer for
those. Deeply version-shaped payloads (charging profiles, composite schedules, local-auth lists,
firmware descriptors, meter values) are carried as `serde_json::Value` inside the neutral params.

**`CsOps` (CS → CSMS, outbound)**

| Method | Versions | Status |
|---|---|:---:|
| `boot_notification` | both | ✅ |
| `heartbeat` | both | ✅ |
| `authorize` | both | ✅ |
| `status_notification` | both | ✅ |
| `meter_values` | both | ✅ |
| `data_transfer` | both | ✅ |
| `firmware_status_notification` | both | ✅ |
| `start_transaction` | merged | ✅ |
| `stop_transaction` | merged | ✅ |
| `notify_event` | v2.0.1-only | ✅ |

**`CsHandler` (CSMS → CS, inbound)**

| Method | Versions | Status |
|---|---|:---:|
| `on_change_availability` | both | ✅ |
| `on_reset` | both | ✅ |
| `on_unlock_connector` | both | ✅ |
| `on_trigger_message` | both | ✅ |
| `on_clear_cache` | both | ✅ |
| `on_get_local_list_version` | both | ✅ |
| `on_cancel_reservation` | both | ✅ |
| `on_set_charging_profile` | both | ✅ |
| `on_clear_charging_profile` | both | ✅ |
| `on_get_composite_schedule` | both | ✅ |
| `on_reserve_now` | both | ✅ |
| `on_send_local_list` | both | ✅ |
| `on_update_firmware` | both | ✅ |
| `on_data_transfer` | both | ✅ |
| `on_set_config` | merged | ✅ |
| `on_get_config` | merged | ✅ |
| `on_start_transaction_requested` | merged | ✅ |
| `on_stop_transaction_requested` | merged | ✅ |

**`CsmsHandler` (CS → CSMS, inbound — mirrors `CsOps`)**

| Method | Versions | Status |
|---|---|:---:|
| `on_boot_notification` | both | ✅ |
| `on_heartbeat` | both | ✅ |
| `on_authorize` | both | ✅ |
| `on_status_notification` | both | ✅ |
| `on_meter_values` | both | ✅ |
| `on_data_transfer` | both | ✅ |
| `on_firmware_status_notification` | both | ✅ |
| `on_start_transaction` | merged | ✅ |
| `on_stop_transaction` | merged | ✅ |
| `on_notify_event` | v2.0.1-only | ✅ |

**`CsmsOps` (CSMS → CS, outbound — mirrors `CsHandler`)**

| Method | Versions | Status |
|---|---|:---:|
| `change_availability` | both | ✅ |
| `reset` | both | ✅ |
| `unlock_connector` | both | ✅ |
| `trigger_message` | both | ✅ |
| `clear_cache` | both | ✅ |
| `get_local_list_version` | both | ✅ |
| `cancel_reservation` | both | ✅ |
| `set_charging_profile` | both | ✅ |
| `clear_charging_profile` | both | ✅ |
| `get_composite_schedule` | both | ✅ |
| `reserve_now` | both | ✅ |
| `send_local_list` | both | ✅ |
| `update_firmware` | both | ✅ |
| `data_transfer` | both | ✅ |
| `set_config` | merged | ✅ |
| `get_config` | merged | ✅ |
| `request_start_transaction` | merged | ✅ |
| `stop_transaction_requested` | merged | ✅ |

### Merged actions

Operations whose wire representation genuinely differs between versions; the semantic layer
presents each as a single method.

| Semantic method | Direction | v1.6 wire action | v2.0.1 wire action | Status |
|---|---|---|---|:---:|
| `start_transaction` | CS → CSMS | `StartTransaction` | `TransactionEvent` (`Started`) | ✅ |
| `stop_transaction` | CS → CSMS | `StopTransaction` | `TransactionEvent` (`Ended`) | ✅ |
| `on_set_config` / `set_config` | CSMS → CS | `ChangeConfiguration` (fan-out) | `SetVariables` (batch) | ✅ |
| `on_get_config` / `get_config` | CSMS → CS | `GetConfiguration` | `GetVariables` | ✅ |
| `on_start_transaction_requested` / `request_start_transaction` | CSMS → CS | `RemoteStartTransaction` | `RequestStartTransaction` | ✅ |
| `on_stop_transaction_requested` / `stop_transaction_requested` | CSMS → CS | `RemoteStopTransaction` | `RequestStopTransaction` | ✅ |

## Action reference

`✅` in **Validated** means the request type derives `validator::Validate` (constraint checks run
before dispatch). `✅` in **Also in** means the action exists in the other OCPP version too. The
`rust_ocpp` module column is the source module path (note it can differ from the wire name, e.g.
v1.6 `heart_beat` → `Heartbeat`).

### OCPP 1.6 — 28 actions

| # | Wire action | `rust_ocpp` module | Validated | Also in |
|---:|---|---|:---:|:---:|
| 1 | `Authorize` | `authorize` | ✅ | ✅ |
| 2 | `BootNotification` | `boot_notification` | ✅ | ✅ |
| 3 | `CancelReservation` | `cancel_reservation` | — | ✅ |
| 4 | `ChangeAvailability` | `change_availability` | — | ✅ |
| 5 | `ChangeConfiguration` | `change_configuration` | ✅ | — |
| 6 | `ClearCache` | `clear_cache` | — | ✅ |
| 7 | `ClearChargingProfile` | `clear_charging_profile` | — | ✅ |
| 8 | `DataTransfer` | `data_transfer` | ✅ | ✅ |
| 9 | `DiagnosticsStatusNotification` | `diagnostics_status_notification` | — | — |
| 10 | `FirmwareStatusNotification` | `firmware_status_notification` | — | ✅ |
| 11 | `GetCompositeSchedule` | `get_composite_schedule` | — | ✅ |
| 12 | `GetConfiguration` | `get_configuration` | ✅ | — |
| 13 | `GetDiagnostics` | `get_diagnostics` | ✅ | — |
| 14 | `GetLocalListVersion` | `get_local_list_version` | — | ✅ |
| 15 | `Heartbeat` | `heart_beat` | ✅ | ✅ |
| 16 | `MeterValues` | `meter_values` | — | ✅ |
| 17 | `RemoteStartTransaction` | `remote_start_transaction` | ✅ | — |
| 18 | `RemoteStopTransaction` | `remote_stop_transaction` | — | — |
| 19 | `ReserveNow` | `reserve_now` | ✅ | ✅ |
| 20 | `Reset` | `reset` | — | ✅ |
| 21 | `SendLocalList` | `send_local_list` | — | ✅ |
| 22 | `SetChargingProfile` | `set_charging_profile` | — | ✅ |
| 23 | `StartTransaction` | `start_transaction` | ✅ | — |
| 24 | `StatusNotification` | `status_notification` | ✅ | ✅ |
| 25 | `StopTransaction` | `stop_transaction` | ✅ | — |
| 26 | `TriggerMessage` | `trigger_message` | — | ✅ |
| 27 | `UnlockConnector` | `unlock_connector` | ✅ | ✅ |
| 28 | `UpdateFirmware` | `update_firmware` | ✅ | ✅ |

### OCPP 2.0.1 — 64 actions

| # | Wire action | `rust_ocpp` module | Validated | Also in |
|---:|---|---|:---:|:---:|
| 1 | `Authorize` | `authorize` | ✅ | ✅ |
| 2 | `BootNotification` | `boot_notification` | — | ✅ |
| 3 | `CancelReservation` | `cancel_reservation` | — | ✅ |
| 4 | `CertificateSigned` | `certificate_signed` | ✅ | — |
| 5 | `ChangeAvailability` | `change_availability` | — | ✅ |
| 6 | `ClearCache` | `clear_cache` | — | ✅ |
| 7 | `ClearChargingProfile` | `clear_charging_profile` | — | ✅ |
| 8 | `ClearDisplayMessage` | `clear_display_message` | — | — |
| 9 | `ClearVariableMonitoring` | `clear_variable_monitoring` | — | — |
| 10 | `ClearedChargingLimit` | `cleared_charging_limit` | — | — |
| 11 | `CostUpdated` | `cost_updated` | ✅ | — |
| 12 | `CustomerInformation` | `customer_information` | ✅ | — |
| 13 | `DataTransfer` | `datatransfer` | ✅ | ✅ |
| 14 | `DeleteCertificate` | `delete_certificate` | — | — |
| 15 | `FirmwareStatusNotification` | `firmware_status_notification` | — | ✅ |
| 16 | `Get15118EVCertificate` | `get_15118ev_certificate` | ✅ | — |
| 17 | `GetBaseReport` | `get_base_report` | — | — |
| 18 | `GetCertificateStatus` | `get_certificate_status` | — | — |
| 19 | `GetChargingProfiles` | `get_charging_profiles` | — | — |
| 20 | `GetCompositeSchedule` | `get_composite_schedule` | — | ✅ |
| 21 | `GetDisplayMessages` | `get_display_message` | — | — |
| 22 | `GetInstalledCertificateIds` | `get_installed_certificate_ids` | — | — |
| 23 | `GetLocalListVersion` | `get_local_list_version` | — | ✅ |
| 24 | `GetLog` | `get_log` | — | — |
| 25 | `GetMonitoringReport` | `get_monitoring_report` | — | — |
| 26 | `GetReport` | `get_report` | ✅ | — |
| 27 | `GetTransactionStatus` | `get_transaction_status` | — | — |
| 28 | `GetVariables` | `get_variables` | — | — |
| 29 | `Heartbeat` | `heartbeat` | — | ✅ |
| 30 | `InstallCertificate` | `install_certificate` | ✅ | — |
| 31 | `LogStatusNotification` | `log_status_notification` | — | — |
| 32 | `MeterValues` | `meter_values` | — | ✅ |
| 33 | `NotifyChargingLimit` | `notify_charging_limit` | — | — |
| 34 | `NotifyCustomerInformation` | `notify_customer_information` | — | — |
| 35 | `NotifyDisplayMessages` | `notify_display_messages` | — | — |
| 36 | `NotifyEVChargingNeeds` | `notify_ev_charging_needs` | — | — |
| 37 | `NotifyEVChargingSchedule` | `notify_ev_charging_schedule` | — | — |
| 38 | `NotifyEvent` | `notify_event` | — | — |
| 39 | `NotifyMonitoringReport` | `notify_monitoring_report` | — | — |
| 40 | `NotifyReport` | `notify_report` | — | — |
| 41 | `PublishFirmware` | `publish_firmware` | — | — |
| 42 | `PublishFirmwareStatusNotification` | `publish_firmware_status_notification` | — | — |
| 43 | `ReportChargingProfiles` | `report_charging_profiles` | — | — |
| 44 | `RequestStartTransaction` | `request_start_transaction` | — | — |
| 45 | `RequestStopTransaction` | `request_stop_transaction` | — | — |
| 46 | `ReservationStatusUpdate` | `reservation_status_update` | — | — |
| 47 | `ReserveNow` | `reserve_now` | — | ✅ |
| 48 | `Reset` | `reset` | — | ✅ |
| 49 | `SecurityEventNotification` | `security_event_notification` | — | — |
| 50 | `SendLocalList` | `send_local_list` | — | ✅ |
| 51 | `SetChargingProfile` | `set_charging_profile` | — | ✅ |
| 52 | `SetDisplayMessage` | `set_display_message` | — | — |
| 53 | `SetMonitoringBase` | `set_monitoring_base` | — | — |
| 54 | `SetMonitoringLevel` | `set_monitoring_level` | — | — |
| 55 | `SetNetworkProfile` | `set_network_profile` | — | — |
| 56 | `SetVariableMonitoring` | `set_variable_monitoring` | — | — |
| 57 | `SetVariables` | `set_variables` | — | — |
| 58 | `SignCertificate` | `sign_certificate` | — | — |
| 59 | `StatusNotification` | `status_notification` | — | ✅ |
| 60 | `TransactionEvent` | `transaction_event` | — | — |
| 61 | `TriggerMessage` | `trigger_message` | — | ✅ |
| 62 | `UnlockConnector` | `unlock_connector` | — | ✅ |
| 63 | `UnpublishFirmware` | `unpublish_firmware` | — | — |
| 64 | `UpdateFirmware` | `update_firmware` | — | ✅ |

## Features

| Feature | Effect |
|---|---|
| `v1_6` (default) | Enables the OCPP 1.6 action set and `V1_6` (forwards to `rust-ocpp/v1_6`). |
| `v2_0_1` (default) | Enables the OCPP 2.0.1 action set and `V2_0_1` (forwards to `rust-ocpp/v2_0_1`). |

Both can be enabled together; either can be built alone.

## Testing

```sh
cargo test  -p ferrowl-ocpp --features v1_6,v2_0_1
cargo build -p ferrowl-ocpp --no-default-features --features v1_6
cargo build -p ferrowl-ocpp --no-default-features --features v2_0_1
cargo clippy -p ferrowl-ocpp --all-features --all-targets
```

[`Version`]: crate::Version
[`Error`]: crate::Error
[`CallError`]: crate::CallError
[`CsActionHandler`]: crate::cs::CsActionHandler
[`CsmsActionHandler`]: crate::csms::CsmsActionHandler
[`CsOps`]: crate::cs::CsOps
[`CsHandler`]: crate::cs::CsHandler
[`CsmsOps`]: crate::csms::CsmsOps
[`CsmsHandler`]: crate::csms::CsmsHandler
