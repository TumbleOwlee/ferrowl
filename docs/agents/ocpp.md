# OCPP domain

Crate: `ferrowl-ocpp`. App-level wiring: `ferrowl/src/module/ocpp/`.

Snapshot: v0.4.13. Update this file when behavior it documents changes (see root `CONTRIBUTING.md`).

> **`ferrowl-ocpp/README.md` is stale**: it documents only v1.6 (28 actions) and v2.0.1 (64 actions). `Cargo.toml` ships `default = ["v1_6", "v2_0_1", "v2_1"]` and the crate implements a full 90-action OCPP 2.1 table (`action/v2_1.rs`). Trust the code, not that README, until it's fixed.

## 1. Version abstraction

`trait Version` (`ferrowl-ocpp/src/action/mod.rs:42-90`), implemented once per version via the `define_ocpp_version!` macro (`ferrowl-ocpp/src/action/macros.rs:33-155`), invoked in `action/v1_6.rs`, `action/v2_0_1.rs`, `action/v2_1.rs`. Each generates a zero-sized marker type (`V1_6`, `V2_0_1`, `V2_1`) plus `Action`/`Response` enums (one variant per action, wrapping `rust_ocpp`'s boxed request/response struct untouched).

| Method | Purpose |
|---|---|
| `action_name(&Action) -> &'static str` | wire name for a variant |
| `action_names() -> &'static [&'static str]` | full table, declaration order |
| `cs_actions() -> &'static [&'static str]` | wire names a CS may originate |
| `csms_actions() -> &'static [(&'static str, ConnectorScope)]` | wire names a CSMS may originate, tagged with connector-targeting shape |
| `default_action`/`default_response(name)` | `Default`-derived template for UI prefill |
| `subprotocol()` | websocket subprotocol token: `"ocpp1.6"`/`"ocpp2.0.1"`/`"ocpp2.1"` |
| `decode_call`/`validate`/`encode_response`/`encode_action`/`decode_result` | wire (de)serialization + `validator::Validate` dispatch |

`ConnectorScope`: `None` (CS-wide only) / `Optional` (shown at CS and connector level) / `Required` (connector-only) — drives the server UI's action-button split.

Version-generic core code (`conn.rs`, `cs/core.rs`, `csms/core.rs`) is monomorphized over `V: Version`; the entire duplex engine, correlation, and command loops are written once, instantiated per version. `V1_6`=28 actions, `V2_0_1`=64, `V2_1`=90 (strict superset: 64 shared + 26 new).

Each version file has a macro-generated `#[cfg(test)] mod ut_validate_flags` asserting the hand-maintained validate flag per table row matches the request type's real `validator::Validate` impl, via autoref-specialization — guards against drift between the table and `rust_ocpp`.

## 2. Roles: CS (client) and CSMS (server)

Both share one duplex engine, `ferrowl-ocpp/src/conn.rs`. Per connection, three concurrent tasks:

- **writer task** — owns the ws sink, drains an mpsc (`OUTBOUND_CHANNEL_CAP = 64`, `ferrowl-ocpp/src/conn.rs:30`, applied at `:142`).
- **reader task** — owns the ws stream; completes `PendingCalls` on `CallResult`/`CallError`; for each inbound `Call`, **spawns a fresh tokio task** running `dispatch_call` so a slow handler never blocks the read pump.
- **per-call awaiter task** (spawned inside `OutboundHandle::call`) — owns the originating `Action` (a `CallResult` payload carries no action name) and applies `timeout` before decoding the reply.

Correlation (`ferrowl-ocpp/src/correlation.rs`): `PendingCalls = Arc<Mutex<HashMap<UniqueId, oneshot::Sender<CallOutcome>>>>` (parking_lot). `fail_all` (`ferrowl-ocpp/src/correlation.rs:52`) runs on `Connection::shutdown`, unblocking every pending caller with a `GenericError` `CallError`.

### CS (client) — `ferrowl-ocpp/src/cs/`

- `ClientBuilder<V>` (`ferrowl-ocpp/src/cs/mod.rs:29`), `::spawn(handler, log)`: builds an HTTP upgrade request, sets `Sec-WebSocket-Protocol: V::subprotocol()`, optionally injects Basic Auth + rustls `Connector` from `CsTlsConfig`, dials with `connect_async_tls_with_config`, spawns `core::run`, returns a `Client<V>` (command channel cap `COMMAND_CHANNEL_CAP = 32`, `ferrowl-ocpp/src/cs/mod.rs:26`).
- `Client<V>`: `call` (await typed reply), `notify` (fire-and-forget), `call_via` (off a cloned sender, doesn't block caller), `terminate`/`join`.
- `core::run`: starts `Connection::start`, calls `handler.on_connected()`, loops `select!` on shutdown notify vs command channel (`SendAction`→fire, `SendActionAwait`→call, `Terminate`/close→break), then `connection.shutdown().await` + `handler.on_disconnected()`.
- `Config` (`ferrowl-ocpp/src/cs/config.rs`): `url` (identity = URL's last path segment, by convention), `timeout_ms` (default 30 000), `basic_auth: Option<BasicAuth>`, `tls: Option<CsTlsConfig>`.

### CSMS (server) — `ferrowl-ocpp/src/csms/`

- `ServerBuilder<V>` (`ferrowl-ocpp/src/csms/mod.rs:39`), `::spawn`: binds `TcpListener`, optional `rustls::ServerConfig` from `CsmsTlsConfig`, creates `ConnectionRegistry<V>`, spawns `accept_loop`.
- `accept_loop`: `select!`s accept vs command channel. On accept: optional TLS-terminate, `accept_hdr_async` with a callback that (a) captures identity from URL path (last non-empty segment), (b) enforces Basic Auth if configured (401 reject), (c) enforces subprotocol match (400 reject), (d) echoes `Sec-WebSocket-Protocol`. Then assigns a fresh `ConnectionId`, registers a per-connection command channel, spawns `core::run_connection`.
- Server `Command`: `Terminate` (broadcast to all), `SendToConnection`/`SendToConnectionAwait` (by `ConnectionId`), `Broadcast` (fan out to all senders), `DisconnectConnection`.
- `ConnectionRegistry<V>` (`ferrowl-ocpp/src/csms/registry.rs:35`): `ConnectionId(u64)` (`ferrowl-ocpp/src/csms/registry.rs:17`) opaque, monotonic `AtomicU64` starting at 1 — deliberately **not** keyed by charge-point identity, so reconnects/duplicate identities don't collide. `RwLock<HashMap<ConnectionId, ConnectionHandle<V>>>` where `ConnectionHandle{cmd_tx, identity: Option<String>}`.
- Per-connection loop `run_connection` mirrors the CS loop, also deregisters on exit, binds the originating `ConnectionId` into dispatch (`CsmsDispatch`) so `CsmsActionHandler::handle_call(conn, action)` can distinguish connections.
- `Config` (`ferrowl-ocpp/src/csms/config.rs`): `host`, `port` (0 = OS-assigned, via `Server::local_addr()`), `timeout_ms` (default 30 000), `basic_auth`, `tls`.

## 3. Exhaustive action lists per version

Verified directly against `ferrowl-ocpp/src/action/{v1_6,v2_0_1,v2_1}.rs`'s `cs=[...]`/`csms=[...]` table declarations.

### OCPP 1.6 — 28 actions

**CS→CSMS (10)**: Authorize, BootNotification, DataTransfer, DiagnosticsStatusNotification, FirmwareStatusNotification, Heartbeat, MeterValues, StartTransaction, StatusNotification, StopTransaction.

**CSMS→CS (18)**, with scope:

| Action | Scope | Purpose |
|---|---|---|
| CancelReservation | None | cancel a prior `ReserveNow` |
| ChangeAvailability | Required | set a connector Operative/Inoperative |
| ChangeConfiguration | None | set one configuration key |
| ClearCache | None | clear CS's local auth cache |
| ClearChargingProfile | Optional | remove installed charging profile(s) |
| GetCompositeSchedule | Required | resolved charging schedule of a connector |
| GetConfiguration | None | read configuration keys |
| GetDiagnostics | None | trigger diagnostics upload |
| GetLocalListVersion | None | read local-auth-list version |
| RemoteStartTransaction | Optional | remotely start a transaction |
| RemoteStopTransaction | None | remotely stop a transaction |
| ReserveNow | Required | reserve a connector |
| Reset | None | soft/hard reset the CS |
| SendLocalList | None | push a local auth list (full/differential) |
| SetChargingProfile | Required | install a charging profile |
| TriggerMessage | Optional | ask CS to (re)send a specific message type |
| UnlockConnector | Required | remotely unlock a connector |
| UpdateFirmware | None | trigger firmware download/install |

### OCPP 2.0.1 — 64 actions

**CS→CSMS (25)**: Authorize, BootNotification, ClearedChargingLimit, DataTransfer, FirmwareStatusNotification, Get15118EVCertificate, GetCertificateStatus, Heartbeat, LogStatusNotification, MeterValues, NotifyChargingLimit, NotifyCustomerInformation, NotifyDisplayMessages, NotifyEVChargingNeeds, NotifyEVChargingSchedule, NotifyEvent, NotifyMonitoringReport, NotifyReport, PublishFirmwareStatusNotification, ReportChargingProfiles, ReservationStatusUpdate, SecurityEventNotification, SignCertificate, StatusNotification, TransactionEvent.

**CSMS→CS (39)**, with scope:

| Action | Scope | Purpose |
|---|---|---|
| CancelReservation | None | cancel a reservation |
| CertificateSigned | None | install a signed cert from `SignCertificate` |
| ChangeAvailability | Optional | set EVSE/connector/CS Operative/Inoperative |
| ClearCache | None | clear auth cache |
| ClearChargingProfile | None | remove charging profile(s) |
| ClearDisplayMessage | None | remove a display message |
| ClearVariableMonitoring | None | remove variable monitors |
| CostUpdated | None | push running-cost update to a live transaction |
| CustomerInformation | None | request/clear customer data |
| DeleteCertificate | None | delete an installed certificate |
| GetBaseReport | None | request a base inventory report |
| GetChargingProfiles | Optional | list installed charging profiles |
| GetCompositeSchedule | Required | resolved schedule for an EVSE |
| GetDisplayMessages | None | list configured display messages |
| GetInstalledCertificateIds | None | list installed certificate IDs |
| GetLocalListVersion | None | read local list version |
| GetLog | None | request diagnostics/security log upload |
| GetMonitoringReport | None | request variable-monitoring report |
| GetReport | None | request a variable/component report |
| GetTransactionStatus | None | query transaction status |
| GetVariables | None | read device model variables |
| InstallCertificate | None | install a CA/root certificate |
| PublishFirmware | None | publish firmware for local distribution |
| RequestStartTransaction | Optional | remote-start (2.0.1 shape) |
| RequestStopTransaction | None | remote-stop (2.0.1 shape) |
| ReserveNow | Optional | reserve an EVSE |
| Reset | Optional | reset CS or EVSE |
| SendLocalList | None | push local auth list |
| SetChargingProfile | Required | install a charging profile |
| SetDisplayMessage | None | configure a display message |
| SetMonitoringBase | None | set monitoring base (All/FactoryDefault/HardWiredOnly) |
| SetMonitoringLevel | None | set monitoring severity threshold |
| SetNetworkProfile | None | configure a network connection profile |
| SetVariableMonitoring | None | add/update variable monitors |
| SetVariables | None | write device model variables |
| TriggerMessage | Optional | request re-send of a specific message |
| UnlockConnector | Required | remote unlock |
| UnpublishFirmware | None | withdraw published firmware |
| UpdateFirmware | None | trigger firmware update |

### OCPP 2.1 — 90 actions with request/response pairs (+1 fire-and-forget datagram)

2.1 = the full 64 shared 2.0.1 actions (same semantics) **plus 26 new**. `NotifyPeriodicEventStream` has no req/resp pair and is intentionally excluded from the action table.

**CS→CSMS new in 2.1 (12)**:

| Action | Purpose |
|---|---|
| BatterySwap | report a battery-swap event (in/out) |
| GetCertificateChainStatus | check OCSP/CRL status of a certificate chain |
| NotifyDERAlarm | report a DER alarm/fault |
| NotifyDERStartStop | report DER control start/stop |
| NotifyPriorityCharging | report priority-charging activation state |
| NotifySettlement | report a payment settlement result |
| NotifyWebPaymentStarted | report a web-payment flow started at the EVSE |
| OpenPeriodicEventStream | open a periodic telemetry stream |
| ClosePeriodicEventStream | close a periodic telemetry stream |
| PullDynamicScheduleUpdate | pull a dynamic charging-schedule update |
| ReportDERControl | report installed DER control curves/settings |
| VatNumberValidation | validate a VAT number for billing |

**CSMS→CS new in 2.1 (14)**:

| Action | Scope | Purpose |
|---|---|---|
| AFRRSignal | None | push an automatic-frequency-restoration-reserve signal |
| AdjustPeriodicEventStream | None | reconfigure an open telemetry stream |
| ChangeTransactionTariff | None | change the tariff applied to a live transaction |
| ClearDERControl | None | remove DER control settings |
| ClearTariffs | Optional | remove tariff(s) |
| GetDERControl | None | read DER control settings |
| GetPeriodicEventStream | None | query telemetry-stream state |
| GetTariffs | Required | read tariff(s) for an EVSE |
| NotifyAllowedEnergyTransfer | None | inform CS which energy-transfer modes are allowed |
| RequestBatterySwap | None | request a battery swap |
| SetDERControl | None | configure DER control settings |
| SetDefaultTariff | Required | set default tariff for an EVSE |
| UpdateDynamicSchedule | None | push a dynamic-schedule update |
| UsePriorityCharging | None | activate/deactivate priority charging |

Shared 64 actions have the same names/purposes as the 2.0.1 table; wire shape may add optional fields (decode-compatible per the crate's own doc comment).

## 4. Handler traits and version sharing

> There is **no** version-neutral semantic layer. An earlier `ferrowl-ocpp/src/semantic/` (`CsOps`/`CsHandler`/`SemanticAdapter`/neutral types) was deliberately removed — the UI is built directly on the per-version action layer so it can list every action and show raw request/response JSON. Don't reintroduce a neutral abstraction without discussing it; the removal was the point.

The only handler traits are version-generic over `V: Version`, each with one required method plus two default lifecycle hooks:

| Trait | Location | Required method (all `-> impl Future<…> + Send`) |
|---|---|---|
| `CsActionHandler<V>` | `ferrowl-ocpp/src/cs/action_handler.rs:13` | `handle_call(&self, action: V::Action) -> Result<V::Response, CallError>` |
| `CsmsActionHandler<V>` | `ferrowl-ocpp/src/csms/action_handler.rs:14` | `handle_call(&self, conn: ConnectionId, action: V::Action) -> Result<V::Response, CallError>` |

Handlers are async and may reject a Call by returning `Err(CallError)`. Both traits default `on_connected`/`on_disconnected` to no-ops (`ferrowl-ocpp/src/cs/action_handler.rs:19`/`:23`, `ferrowl-ocpp/src/csms/action_handler.rs:21`/`:25`). The CSMS variant threads `ConnectionId` so one handler can serve many stations.

`ClientVersion` (`ferrowl/src/module/ocpp/client/view/mod.rs:102`) and `ServerVersion` (`ferrowl/src/module/ocpp/server/view/mod.rs:122`) are the app-level per-version glue traits (both `: Version`).

**App-level implementations are per-version**, in `ferrowl/src/module/ocpp/{client,server}/v{1_6,2_0_1,2_1}/handler.rs`. Version portability is achieved by **code sharing, not abstraction**:

- `ferrowl/src/module/ocpp/client/v2_common.rs` and `ferrowl/src/module/ocpp/server/v2_common.rs` hold the 2.x bodies as plain free functions; `v2_0_1` and `v2_1` each `impl ClientVersion`/`ServerVersion` by delegating to them. 2.1 is a strict superset of 2.0.1 and answers the same core Calls identically.
- The inbound handler type itself is **not** shared: it is typed over the version's own `rust_ocpp`-derived `Action`/`Response` enums, so its concrete type differs per version even though the decision logic is identical. Only the version-independent helpers it calls live in `v2_common.rs` — server side: `cs_status` (`ferrowl/src/module/ocpp/server/v2_common.rs:24`), `evse_status` (`:33`), `craft_response` (`:44`).
- 2.0.1 and 2.1 share one state type: `CsState` (`ferrowl/src/module/ocpp/client/v2_0_1/state.rs:347`) on the client, `server/v2_0_1/state.rs` on the server.

So the two real per-version seams are: the inbound handler type, and the action-spec module (§7). v1.6 shares nothing with 2.x.

## 5. App-level OCPP module config (`ferrowl/src/module/ocpp/config/`)

Two-file split mirroring Modbus: a session-level `OcppModuleSpec` (endpoint + referenced device file) plus a device-type `OcppDeviceConfig` (version/role/timeout/scripts/security/persisted state).

### `OcppSpec` (runtime, merges both) — `ferrowl/src/module/ocpp/config/session.rs:89`

| Field | Type | Notes |
|---|---|---|
| `name` | String | |
| `version` | `OcppVersion` | `V1_6` (default) / `V2_0_1` / `V2_1`; serde tags `"1.6"`/`"2.0.1"`/`"2.1"` |
| `role` | `OcppRole` | `Client` (default, CS) / `Server` (CSMS) |
| `protocol` | `OcppProtocol` | `Ws` (default) / `Wss` |
| `ip` | String | host |
| `port` | u16 | |
| `path` | String | URL path segment, e.g. `/ocpp/cp001`; conventionally carries charge-point identity |
| `timeout_ms` | `Option<u64>` | `None` → crate default 30 000 |
| `security` | `OcppSecurityConfig` | see below |

`OcppSpec::url()` (`ferrowl/src/module/ocpp/config/session.rs:113`) = `{protocol}{ip}:{port}{path}`. `OcppSpec::effective_csms_tls()` (`:122`): for role=Server + `Wss`, uses configured TLS material, else falls back to an **ephemeral self-signed** cert (never silently binds plain TCP for a `wss://`-labeled instance) — `csms_self_signed_fallback()` (`ferrowl/src/module/ocpp/config/session.rs:136`) reports when this happened, surfaced in the module log.

### `OcppModuleSpec` (session-persisted) — `ferrowl/src/module/ocpp/config/session.rs:161`

`name`, `device` (path to device-config file), `protocol`, `ip`, `port`, `path`. No version/role/timeout here — those live only in the device file.

### `OcppSecurityConfig` — `ferrowl/src/module/ocpp/config/device.rs:20`

`username`/`password` (Basic Auth, Profile 1), `ca_file` (client trust anchor), `cert_file`+`key_file` (server cert chain — presence of both turns on TLS for the listener), `client_cert_file`+`client_key_file` (client mTLS cert), `client_ca_file` (server-side client-cert verification CA), `require_client_cert: bool` (Profile 3), `self_signed: bool` (server ephemeral cert fallback; explicit files always win), `insecure_skip_verify: bool` (client accepts any server cert — test rigs only).

### `OcppDeviceConfig` (device-type file) — `ferrowl/src/module/ocpp/config/device.rs:158`

| Field | Type | Notes |
|---|---|---|
| `version` | `Option<String>` | ferrowl version stamp on save |
| `ocpp_version` | `OcppVersion` | |
| `role` | `OcppRole` | |
| `timeout_ms` | `Option<u64>` | |
| `scripts` | `Vec<ScriptDef>` | Lua sim scripts, client role only (see `lua.md`) |
| `script_interval` | f64 | default 1.0s, floored to `MIN_SCRIPT_INTERVAL_SECS = 0.05s`, NaN/∞/≤0 also fall back to 1.0s |
| `log_file` | `Option<String>` | `:log <file>` persistent sink base |
| `rfids` | `Vec<String>` | server-role charge-point-wide RFID accept-list; empty = accept all |
| `connector_rfids` | `Vec<ConnectorRfids>` | per-connector accept-lists, unioned with `rfids` |
| `connectors` | `Vec<ConnectorRef>` | client-role connector table seed; **no max-count constant** |
| `config` | `Vec<ConfigKeyDef>` | client-role persisted GetConfiguration/GetVariables key store |
| `security` | `OcppSecurityConfig` | |

`ConnectorRef{evse: Option<i64>, connector: i64}` — `evse: None` for 1.6 (connector-only addressing), `Some` for 2.0.1/2.1 (EVSE+connector). `ConfigKeyDef{key, value, readonly}`.

### Setup dialog fields (`ferrowl/src/module/ocpp/setup_dialog.rs`)

name (default `"cs-1"`), config path (device file), version selector, role selector, protocol selector, ip (default `127.0.0.1`), port (default `9000`), path (client-role only, default `/ocpp/cp001`), and — only when protocol=`Wss` — a `SecurityLevel` selector (`None`/`BasicAuth`/`Tls`/`MutualTls`, cumulative fields) plus username/password/skip_verify/ca_file/cert_file/key_file/client_cert_file/client_key_file/client_ca_file, conditionally focusable per level. No connector-count field — connectors are added later into `OcppDeviceConfig.connectors`.

## 6. Numeric limits / constants

| Constant | Value | Meaning |
|---|---|---|
| `OUTBOUND_CHANNEL_CAP` | 64 | mpsc capacity feeding the writer task |
| `COMMAND_CHANNEL_CAP` (CS) | 32 | client-handle→task command channel |
| `COMMAND_CHANNEL_CAP` (CSMS) | 32 | server command channel + each per-connection channel |
| CS/CSMS `default_timeout_ms` | 30 000 ms | default awaited-Call reply timeout |
| `registry.next_id` seed | 1 (`AtomicU64`) | `ConnectionId` assignment |
| `MAX_MESSAGES` | 200 | in-memory message-log buffer cap per client view (oldest evicted) |
| `TICKS_PER_SEC` | 10 | UI refresh tick rate (~100ms) for seconds→ticks conversion |
| `DEFAULT_HEARTBEAT_SECS` | 30 s | fallback heartbeat cadence until BootNotification supplies one (or supplies 0) |
| `MIN_SCRIPT_INTERVAL_SECS` | 0.05 s | floor for Lua sim cycle interval |
| `default_script_interval()` | 1.0 s | default Lua sim cycle |
| setup dialog default port | 9000 | |
| setup dialog default ip | `127.0.0.1` | |
| max connectors | **no constant** | `connectors: Vec<ConnectorRef>` unbounded |
| Authorize `certificate` cap (2.0.1) | 5500 chars | validation ceiling |
| Authorize `certificate` cap (2.1) | 10 000 chars | validation ceiling |
| DataTransfer `vendor_id` cap (2.0.1) | 255 chars | validation ceiling |

## 7. Raw-JSON editor vs typed send dialog

Each action resolves to either an `ActionSpec` (`ferrowl/src/module/ocpp/action_dialog.rs:120`; per-version spec modules in `ferrowl/src/module/ocpp/spec/`) — a flat property table: Name|Type|Value, `PropKind ∈ {Text, Number, Bool, Enum(&[&str]), Timestamp}` (`ferrowl/src/module/ocpp/action_dialog.rs:72`), `PropSource ∈ {StateField, GeneratedTxId, Now, Constant, Empty}` (`:97`) — or falls back to raw JSON. Rule: typed spec **unless** the payload's required field is itself nested (object) or a required list with no optional escape hatch — those stay JSON-only. A "JSON" toggle always lets the operator switch a typed dialog to raw JSON too. Two `ActionSpec` shapes: `spec()` (flat, `assemble = flat_object`) and `nested()` (`complex: true`, custom `Assembler` folds flat rows into a nested payload).

**OCPP 2.0.1 JSON-only (8)**: `SetNetworkProfile`, `SetVariableMonitoring`, `NotifyEVChargingNeeds`, `NotifyEVChargingSchedule`, `NotifyMonitoringReport`, `NotifyReport`, `ReportChargingProfiles`, `TransactionEvent`.

**OCPP 2.1 additional JSON-only (8)**: `BatterySwap`, `GetCertificateChainStatus`, `OpenPeriodicEventStream`, `ReportDERControl`, `AdjustPeriodicEventStream`, `ChangeTransactionTariff`, `SetDefaultTariff`, `UpdateDynamicSchedule`. 2.1 also overrides one shared template: `NotifyMonitoringReport`'s 2.1 `VariableMonitoringType` additionally requires `eventNotificationType`.

**2.1 typed-dialog additions (12 flat + 2 nested)**: flat CS-originated — `NotifyDERAlarm`, `NotifyDERStartStop`, `NotifyPriorityCharging`, `NotifySettlement`, `NotifyWebPaymentStarted`, `ClosePeriodicEventStream`, `PullDynamicScheduleUpdate`, `VatNumberValidation`; flat CSMS-originated — `AFRRSignal`, `ClearDERControl`, `GetDERControl`, `GetPeriodicEventStream` (empty prop list), `GetTariffs`, `ClearTariffs`, `SetDERControl`, `UsePriorityCharging`; nested — `RequestBatterySwap`, `NotifyAllowedEnergyTransfer`. Everything else among the 64 shared actions delegates unchanged to the 2.0.1 spec module, since 2.1's additions to shared payload types are all optional.

Test invariant enforced for both v2.0.1 and v2.1: every dialog-reachable action (`csms_actions + cs_actions` minus `STATE_DRIVEN` = Authorize/BootNotification/Heartbeat/MeterValues/StatusNotification, which a CS builds from state without a dialog) must be exactly one of {typed spec, JSON-only} — `has_spec ^ is_json`.
