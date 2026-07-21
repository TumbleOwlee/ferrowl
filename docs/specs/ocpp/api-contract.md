# OCPP — API Contract

The stable public surface owned by the OCPP area: the exhaustive action table per
version, the direction each action flows, the OCPP-J error codes, and the OCPP
module configuration fields (including TLS/mTLS and HTTP Basic Auth).

Per the ownership rule in [`../README.md`](../README.md), the OCPP config fields
are specified here, not in `config-session/`. `config-session/` owns only the file
envelope (format, `version`, the session→module list, save/load, `migrate`).

---

## 1. Versions and subprotocols

| Version | Subprotocol token | Actions | CS→CSMS | CSMS→CS |
|---|---|---|---|---|
| 1.6 | `ocpp1.6` | **28** | 10 | 18 |
| 2.0.1 | `ocpp2.0.1` | **64** | 25 | 39 |
| 2.1 | `ocpp2.1` | **90** | 37 | 53 |

2.1 is a strict superset of 2.0.1: the 64 shared actions carry over verbatim,
plus 26 new ones. The one-way streaming datagram `NotifyPeriodicEventStream` has
no request/response pair and is deliberately **not** an action.

The **connector scope** column on CSMS→CS actions is `None` (charge-point-wide
only), `Optional` (usable at both charge-point and connector level), or `Required`
(connector-only). It is derived from the presence and optionality of the request's
*top-level* connector/EVSE target; a nested-optional EVSE field (e.g. inside
charging-profile or variable criteria) counts as `None`.

---

## 2. OCPP 1.6 — 28 actions

### 2.1 CS→CSMS (10)

`Authorize`, `BootNotification`, `DataTransfer`, `DiagnosticsStatusNotification`,
`FirmwareStatusNotification`, `Heartbeat`, `MeterValues`, `StartTransaction`,
`StatusNotification`, `StopTransaction`.

### 2.2 CSMS→CS (18)

| Action | Scope |
|---|---|
| `CancelReservation` | None |
| `ChangeAvailability` | Required |
| `ChangeConfiguration` | None |
| `ClearCache` | None |
| `ClearChargingProfile` | Optional |
| `GetCompositeSchedule` | Required |
| `GetConfiguration` | None |
| `GetDiagnostics` | None |
| `GetLocalListVersion` | None |
| `RemoteStartTransaction` | Optional |
| `RemoteStopTransaction` | None |
| `ReserveNow` | Required |
| `Reset` | None |
| `SendLocalList` | None |
| `SetChargingProfile` | Required |
| `TriggerMessage` | Optional |
| `UnlockConnector` | Required |
| `UpdateFirmware` | None |

---

## 3. OCPP 2.0.1 — 64 actions

### 3.1 CS→CSMS (25)

`Authorize`, `BootNotification`, `ClearedChargingLimit`, `DataTransfer`,
`FirmwareStatusNotification`, `Get15118EVCertificate`, `GetCertificateStatus`,
`Heartbeat`, `LogStatusNotification`, `MeterValues`, `NotifyChargingLimit`,
`NotifyCustomerInformation`, `NotifyDisplayMessages`, `NotifyEVChargingNeeds`,
`NotifyEVChargingSchedule`, `NotifyEvent`, `NotifyMonitoringReport`,
`NotifyReport`, `PublishFirmwareStatusNotification`, `ReportChargingProfiles`,
`ReservationStatusUpdate`, `SecurityEventNotification`, `SignCertificate`,
`StatusNotification`, `TransactionEvent`.

### 3.2 CSMS→CS (39)

| Action | Scope |
|---|---|
| `CancelReservation` | None |
| `CertificateSigned` | None |
| `ChangeAvailability` | Optional |
| `ClearCache` | None |
| `ClearChargingProfile` | None |
| `ClearDisplayMessage` | None |
| `ClearVariableMonitoring` | None |
| `CostUpdated` | None |
| `CustomerInformation` | None |
| `DeleteCertificate` | None |
| `GetBaseReport` | None |
| `GetChargingProfiles` | Optional |
| `GetCompositeSchedule` | Required |
| `GetDisplayMessages` | None |
| `GetInstalledCertificateIds` | None |
| `GetLocalListVersion` | None |
| `GetLog` | None |
| `GetMonitoringReport` | None |
| `GetReport` | None |
| `GetTransactionStatus` | None |
| `GetVariables` | None |
| `InstallCertificate` | None |
| `PublishFirmware` | None |
| `RequestStartTransaction` | Optional |
| `RequestStopTransaction` | None |
| `ReserveNow` | Optional |
| `Reset` | Optional |
| `SendLocalList` | None |
| `SetChargingProfile` | Required |
| `SetDisplayMessage` | None |
| `SetMonitoringBase` | None |
| `SetMonitoringLevel` | None |
| `SetNetworkProfile` | None |
| `SetVariableMonitoring` | None |
| `SetVariables` | None |
| `TriggerMessage` | Optional |
| `UnlockConnector` | Required |
| `UnpublishFirmware` | None |
| `UpdateFirmware` | None |

---

## 4. OCPP 2.1 — 90 actions

All 64 of the 2.0.1 actions listed in §3, plus the 26 new ones below.

**No shared action changes direction or scope in 2.1.** Every one of the 64 keeps
its 2.0.1 name, its 2.0.1 direction, and its 2.0.1 connector scope. 2.1's
additions to the shared payload types are all optional fields, so the shared
actions remain decode-compatible.

### 4.1 New in 2.1, CS→CSMS (12)

`BatterySwap`, `GetCertificateChainStatus`, `NotifyDERAlarm`,
`NotifyDERStartStop`, `NotifyPriorityCharging`, `NotifySettlement`,
`NotifyWebPaymentStarted`, `OpenPeriodicEventStream`, `ClosePeriodicEventStream`,
`PullDynamicScheduleUpdate`, `ReportDERControl`, `VatNumberValidation`.

### 4.2 New in 2.1, CSMS→CS (14)

| Action | Scope |
|---|---|
| `AFRRSignal` | None |
| `AdjustPeriodicEventStream` | None |
| `ChangeTransactionTariff` | None |
| `ClearDERControl` | None |
| `ClearTariffs` | Optional |
| `GetDERControl` | None |
| `GetPeriodicEventStream` | None |
| `GetTariffs` | Required |
| `NotifyAllowedEnergyTransfer` | None |
| `RequestBatterySwap` | None |
| `SetDERControl` | None |
| `SetDefaultTariff` | Required |
| `UpdateDynamicSchedule` | None |
| `UsePriorityCharging` | None |

Totals: 25 + 12 = **37** CS→CSMS; 39 + 14 = **53** CSMS→CS; 37 + 53 = **90**.

---

## 5. OCPP-J CallError codes

The fixed set, spelled exactly as they appear on the wire:

| Code | Emitted when |
|---|---|
| `NotImplemented` | the action name is unknown to the negotiated version, or the peer's simulator does not handle it |
| `NotSupported` | (accepted on the wire; never emitted) |
| `InternalError` | a crafted response failed to encode; a CSMS command named an unknown connection |
| `ProtocolError` | (accepted on the wire; never emitted) |
| `SecurityError` | (accepted on the wire; never emitted) |
| `FormationViolation` | the Call payload failed to deserialize, or failed the version's validation rules |
| `PropertyConstraintViolation` | an inbound Call targets a connector/EVSE this charging station does not have |
| `OccurenceConstraintViolation` | (accepted on the wire; never emitted — note the spec's own spelling) |
| `TypeConstraintViolation` | (accepted on the wire; never emitted) |
| `GenericError` | an awaited Call timed out, its connection closed, or its connection was torn down |

An `errorCode` string received that matches none of the ten is accepted and read
as `GenericError`.

---

## 6. Module instance spec (session / `--ocpp`)

One OCPP module instance. This is the per-instance, on-the-wire endpoint; the
version, role, timeout, security, scripts, connectors, configuration keys, and
(client role) CS boot identity all live in the referenced device config (§8),
never here.

| Field | Type | Default | Valid values |
|---|---|---|---|
| `name` | string | — (required) | tab / instance name |
| `device` | string | — (required) | path to the OCPP device config file |
| `protocol` | enum | `ws` | `ws`, `wss` |
| `ip` | string | — (required in the session file; `127.0.0.1` when built from `--ocpp`) | host |
| `port` | u16 | — (required) | 0–65535; `0` in the server role binds an OS-assigned port |
| `path` | string | empty | URL path, e.g. `/ocpp/cp001`. Empty = none |

The dialed/advertised URL is `{protocol}://{ip}:{port}{path}`. The OCPP
charge-point identity is, by convention, the last non-empty segment of `path`.

### `--ocpp` key/value form

`--ocpp name=…,device=…,protocol=…,ip=…,port=…,path=…` accepts the same keys,
with `name`, `device`, and `port` **required**, `ip` defaulting to `127.0.0.1`,
`protocol` defaulting to `ws`, and `path` defaulting to empty. A `protocol` other
than `ws`/`wss` is an error.

---

## 7. Endpoint enums

| Enum | Values | Serialized as |
|---|---|---|
| `ocpp_version` | 1.6 (default), 2.0.1, 2.1 | `"1.6"`, `"2.0.1"`, `"2.1"` |
| `role` | client (default), server | `"client"`, `"server"` |
| `protocol` | ws (default), wss | `"ws"`, `"wss"` |

`client` is the Charging Station (CS) role; `server` is the CSMS role.

---

## 8. Device config (one file = one device type)

| Field | Type | Default | Notes |
|---|---|---|---|
| `version` | optional string | unset | ferrowl version, stamped on save |
| `ocpp_version` | enum | `1.6` | see §7. Version-locks the file, because scripts call version-specific actions |
| `role` | enum | `client` | see §7 |
| `timeout_ms` | optional u64 | `30000` when unset | awaited-reply timeout, both roles |
| `scripts` | list of script defs | empty | Lua sim scripts — see `scripting/`. Client role only |
| `script_interval` | f64 seconds | `1.0` | Lua sim cycle; floored at `0.05`; NaN/∞/≤0 fall back to `1.0` |
| `log_file` | optional string | unset | persistent log-file base, also set by `:log <file>` |
| `rfids` | list of string | empty | **server only**: charge-point-wide RFID accept-list |
| `connector_rfids` | list of `ConnectorRfids` | empty | **server only**: per-connector accept-lists |
| `connectors` | list of `ConnectorRef` | empty | **client only**: connector-table seed. Empty = CS-level only. Unbounded |
| `config` | list of `ConfigKeyDef` | empty | **client only**: persisted configuration/variable key store. Empty = built-in defaults |
| `model` | optional string | unset | **client only**: CS boot identity model, seeded/written like `connectors`/`config` |
| `vendor` | optional string | unset | **client only**: CS boot identity vendor |
| `firmware_version` | optional string | unset | **client only**: CS boot identity firmware version |
| `serial_number` | optional string | unset | **client only**: CS boot identity serial number |
| `iccid` | optional string | unset | **client only, 1.6 only**: SIM ICCID, seeded/written like `model`/`vendor`. Inert for 2.0.1/2.1 |
| `imsi` | optional string | unset | **client only, 1.6 only**: SIM IMSI |
| `meter_serial_number` | optional string | unset | **client only, 1.6 only**: installed meter's serial number |
| `meter_type` | optional string | unset | **client only, 1.6 only**: installed meter's type/model |
| `security` | `OcppSecurityConfig` | all-unset | §9 |

A device config file written before any of these fields existed still loads: every
field is defaulted.

### 8.1 `ConnectorRef`

| Field | Type | Notes |
|---|---|---|
| `evse` | optional i64 | `None` for 1.6 (connector-only addressing); `Some` for 2.0.1/2.1 |
| `connector` | i64 | connector id |

### 8.2 `ConnectorRfids`

| Field | Type | Notes |
|---|---|---|
| `evse` | optional i64 | as above |
| `connector` | optional i64 | as above |
| `rfids` | list of string | tags accepted for that connector, **in addition to** the inherited charge-point-wide list |

### 8.3 `ConfigKeyDef`

| Field | Type | Default |
|---|---|---|
| `key` | string | — (required) |
| `value` | string | empty |
| `readonly` | bool | `false` |

---

## 9. Security config (`security`)

One section, shared by both roles. A field irrelevant to the instance's role is
simply left unset. All-unset (the default) is plain `ws://` with no auth.

| Field | Type | Default | Role | Meaning |
|---|---|---|---|---|
| `username` | optional string | unset | both | Basic Auth username (Profile 1). Client sends it; server requires it |
| `password` | optional string | unset | both | Basic Auth password. Never logged |
| `ca_file` | optional string | unset | client | extra PEM trust anchor, added on top of the webpki root store |
| `cert_file` | optional string | unset | server | PEM certificate chain presented to clients |
| `key_file` | optional string | unset | server | PEM private key matching `cert_file` |
| `client_cert_file` | optional string | unset | client | PEM client certificate presented for mutual TLS (Profile 3) |
| `client_key_file` | optional string | unset | client | PEM private key matching `client_cert_file` |
| `client_ca_file` | optional string | unset | server | PEM CA used to verify client certificates |
| `require_client_cert` | bool | `false` | server | reject clients without a certificate signed by `client_ca_file` (Profile 3) |
| `self_signed` | bool | `false` | server | generate an ephemeral in-memory certificate instead of loading files |
| `insecure_skip_verify` | bool | `false` | client | accept **any** server certificate without authenticating it. Test rigs only |

### 9.1 Derivation rules

- **Basic Auth is on** iff *both* `username` and `password` are set. Either alone
  is inert.
- **Client TLS is on** iff any of `ca_file`, `client_cert_file`,
  `client_key_file`, `insecure_skip_verify` is set.
- **Client mTLS** requires *both* `client_cert_file` and `client_key_file`; either
  alone presents no client certificate.
- `insecure_skip_verify` **ignores** `ca_file` rather than combining with it.
- The **endpoint scheme gates TLS** in both roles. A `ws://` endpoint is always
  plaintext, and any TLS material configured alongside it is inert. Only a `wss://`
  endpoint uses the fields below. A URL never advertises a transport its peer does
  not speak.
- **A `wss://` server's TLS mode** is: PEM files when *both* `cert_file` and
  `key_file` are set (explicit files always win); otherwise ephemeral self-signed
  when `self_signed` is set.
- **A `wss://` server endpoint with none of the above** falls back to an ephemeral
  self-signed certificate rather than binding plain TCP, and reports the fallback
  in the module log.

### 9.2 Security profiles

| Profile | Configuration |
|---|---|
| 1 — Basic Auth over `ws://` | `username` + `password`, `protocol = ws` |
| 2 — TLS, server cert only | `protocol = wss` + server `cert_file`/`key_file` (or `self_signed`); optionally combined with Basic Auth |
| 3 — mutual TLS | Profile 2, plus server `client_ca_file` + `require_client_cert`, and client `client_cert_file` + `client_key_file` |

---

## 10. `:` commands

Protocol-specific commands owned by this area (the command mechanism itself is
owned by `tui/`).

### 10.1 Client (CS) view

| Command | Effect |
|---|---|
| `:start` | connect to the CSMS |
| `:stop` | disconnect |
| `:restart` | disconnect, then reconnect |
| `:e` / `:edit` | open the module setup dialog |
| `:wd` / `:write-device [path]` | save the device config |
| `:compact` | toggle compact table rows |
| `:log [file]` | set (or, with no argument, clear) the persistent log file |

### 10.2 Server (CSMS) view

| Command | Effect |
|---|---|
| `:start` | bind the listener |
| `:stop` | unbind the listener and discard every observed station entry |
| `:restart` | rebind the listener; discards every observed station entry |
| `:e` / `:edit` | open the module setup dialog |
| `:wd` / `:write-device [path]` | save the device config |
| `:compact` | toggle compact table rows |
| `:log [file]` | set (or clear) the persistent log file |

A client does **not** connect on creation — `:start` is required. A server binds
its listener automatically on creation.
