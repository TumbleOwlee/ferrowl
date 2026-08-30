# OCPP — API Contract

Exhaustive action table per version, direction per action, OCPP-J error codes, OCPP module config fields (TLS/mTLS, HTTP Basic Auth). Per [`../README.md`](../README.md)'s ownership rule, OCPP config fields live here, not `config-session/` (envelope only).

---

## 1. Versions and subprotocols

| Version | Subprotocol token | Actions | CS→CSMS | CSMS→CS |
|---|---|---|---|---|
| 1.6 | `ocpp1.6` | **28** | 10 | 18 |
| 2.0.1 | `ocpp2.0.1` | **64** | 25 | 39 |
| 2.1 | `ocpp2.1` | **90** | 37 | 53 |

2.1 is a strict superset of 2.0.1: 64 shared actions verbatim plus 26 new. The one-way streaming datagram `NotifyPeriodicEventStream` has no request/response pair and is deliberately **not** an action.

**Connector scope** on CSMS→CS actions: `None` (charge-point-wide only), `Optional` (charge-point or connector level), `Required` (connector-only). Derived from the presence and optionality of the request's *top-level* connector/EVSE target; a nested-optional EVSE field (inside charging-profile or variable criteria) counts as `None`.

---

## 2. OCPP 1.6 — 28 actions

### 2.1 CS→CSMS (10)

`Authorize`, `BootNotification`, `DataTransfer`, `DiagnosticsStatusNotification`, `FirmwareStatusNotification`, `Heartbeat`, `MeterValues`, `StartTransaction`, `StatusNotification`, `StopTransaction`.

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

`Authorize`, `BootNotification`, `ClearedChargingLimit`, `DataTransfer`, `FirmwareStatusNotification`, `Get15118EVCertificate`, `GetCertificateStatus`, `Heartbeat`, `LogStatusNotification`, `MeterValues`, `NotifyChargingLimit`, `NotifyCustomerInformation`, `NotifyDisplayMessages`, `NotifyEVChargingNeeds`, `NotifyEVChargingSchedule`, `NotifyEvent`, `NotifyMonitoringReport`, `NotifyReport`, `PublishFirmwareStatusNotification`, `ReportChargingProfiles`, `ReservationStatusUpdate`, `SecurityEventNotification`, `SignCertificate`, `StatusNotification`, `TransactionEvent`.

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

All 64 of §3 plus the 26 below. **No shared action changes direction or scope in 2.1.** 2.1's additions to shared payload types are all optional fields, so shared actions remain decode-compatible.

### 4.1 New in 2.1, CS→CSMS (12)

`BatterySwap`, `GetCertificateChainStatus`, `NotifyDERAlarm`, `NotifyDERStartStop`, `NotifyPriorityCharging`, `NotifySettlement`, `NotifyWebPaymentStarted`, `OpenPeriodicEventStream`, `ClosePeriodicEventStream`, `PullDynamicScheduleUpdate`, `ReportDERControl`, `VatNumberValidation`.

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

Totals: 25 + 12 = **37** CS→CSMS; 39 + 14 = **53** CSMS→CS; **90**.

---

## 5. OCPP-J CallError codes

Fixed set, spelled as on the wire:

| Code | Emitted when |
|---|---|
| `NotImplemented` | action unknown to the negotiated version, or the peer's simulator does not handle it |
| `NotSupported` | (accepted on the wire; never emitted) |
| `InternalError` | a crafted response failed to encode; a CSMS command named an unknown connection |
| `ProtocolError` | (accepted on the wire; never emitted) |
| `SecurityError` | (accepted on the wire; never emitted) |
| `FormationViolation` | Call payload failed to deserialize, or failed the version's validation rules |
| `PropertyConstraintViolation` | inbound Call targets a connector/EVSE the station lacks |
| `OccurenceConstraintViolation` | (accepted on the wire; never emitted — spec's own spelling) |
| `TypeConstraintViolation` | (accepted on the wire; never emitted) |
| `GenericError` | awaited Call timed out, its connection closed, or was torn down |

An `errorCode` matching none of the ten is accepted and read as `GenericError`.

---

## 6. Module instance spec (session / `--ocpp`)

One OCPP instance: the per-instance on-the-wire endpoint. Version, role, timeout, security, scripts, connectors, configuration keys, (client role) CS boot identity live in the device config (§8), never here.

| Field | Type | Default | Valid values |
|---|---|---|---|
| `name` | string | — (required) | tab / instance name |
| `device` | string | — (required) | OCPP device config file path |
| `protocol` | enum | `ws` | `ws`, `wss` |
| `ip` | string | — (required in the session file; `127.0.0.1` from `--ocpp`) | host |
| `port` | u16 | — (required) | 0–65535; `0` in the server role binds an OS-assigned port |
| `path` | string | empty | URL path, e.g. `/ocpp/cp001`. Empty = none |

Dialed/advertised URL: `{protocol}://{ip}:{port}{path}`. Charge-point identity by convention = last non-empty segment of `path`.

### `--ocpp` key/value form

`--ocpp name=…,device=…,protocol=…,ip=…,port=…,path=…` accepts the same keys; `name`, `device`, `port` **required**; `ip` default `127.0.0.1`; `protocol` default `ws`; `path` default empty. `protocol` other than `ws`/`wss` is an error.

---

## 7. Endpoint enums

| Enum | Values | Serialized as |
|---|---|---|
| `ocpp_version` | 1.6 (default), 2.0.1, 2.1 | `"1.6"`, `"2.0.1"`, `"2.1"` |
| `role` | client (default), server | `"client"`, `"server"` |
| `protocol` | ws (default), wss | `"ws"`, `"wss"` |

`client` = Charging Station (CS); `server` = CSMS.

---

## 8. Device config (one file = one device type)

| Field | Type | Default | Notes |
|---|---|---|---|
| `version` | optional string | unset | ferrowl version, stamped on save |
| `ocpp_version` | enum | `1.6` | §7. Version-locks the file: scripts call version-specific actions |
| `role` | enum | `client` | §7 |
| `timeout_ms` | optional u64 | `30000` when unset | awaited-reply timeout, both roles |
| `scripts` | list of script defs | empty | Lua sim scripts — `scripting/`. Client role only |
| `script_interval` | f64 seconds | `1.0` | Lua sim cycle; floored at `0.05`; NaN/∞/≤0 → `1.0` |
| `log_file` | optional string | unset | persistent log-file base, also set by `:log <file>` |
| `rfids` | list of string | empty | **server only**: charge-point-wide RFID accept-list |
| `connector_rfids` | list of `ConnectorRfids` | empty | **server only**: per-connector accept-lists |
| `connectors` | list of `ConnectorRef` | empty | **client only**: connector-table seed. Empty = CS-level only. Unbounded |
| `config` | list of `ConfigKeyDef` | empty | **client only**: persisted configuration/variable key store. Empty = built-in defaults |
| `extra_headers` | list of `HeaderDef` | empty | **client only**: extra HTTP headers on the WebSocket upgrade, in addition to the client's own. OC-R-117–119 |
| `model` | optional string | unset | **client only**: CS boot identity model, seeded/written like `connectors`/`config` |
| `vendor` | optional string | unset | **client only**: CS boot identity vendor |
| `firmware_version` | optional string | unset | **client only**: CS boot identity firmware version |
| `serial_number` | optional string | unset | **client only**: CS boot identity serial number |
| `iccid` | optional string | unset | **client only, 1.6 only**: SIM ICCID, seeded/written like `model`/`vendor`. Inert for 2.0.1/2.1 |
| `imsi` | optional string | unset | **client only, 1.6 only**: SIM IMSI |
| `meter_serial_number` | optional string | unset | **client only, 1.6 only**: installed meter's serial number |
| `meter_type` | optional string | unset | **client only, 1.6 only**: installed meter's type/model |
| `security` | `OcppSecurityConfig` | all-unset | §9 |

A device config written before any of these fields existed still loads: every field defaulted.

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
| `rfids` | list of string | tags accepted for that connector, **in addition to** the charge-point-wide list |

### 8.3 `ConfigKeyDef`

| Field | Type | Default |
|---|---|---|
| `key` | string | — (required) |
| `value` | string | empty |
| `readonly` | bool | `false` |

### 8.4 `HeaderDef`

| Field | Type | Default |
|---|---|---|
| `name` | string | — (required) |
| `value` | string | — (required) |

---

## 9. Security config (`security`)

One section, both roles. Basic Auth role-shared; TLS held per role in a `tls` sub-block (OC-R-126), of which an instance consults only its own role's — the other inert. Default (no auth, both policies `none`) = plain `ws://`.

| Field | Type | Default | Role | Meaning |
|---|---|---|---|---|
| `username` | optional string | unset | both | Basic Auth username (Profile 1). Client sends; server requires |
| `password` | optional string | unset | both | Basic Auth password. Never logged |
| `tls.server` | `ServerTlsPolicy` | `none` | server | consulted when the instance runs as CSMS |
| `tls.client` | `ClientTlsPolicy` | `none` | client | consulted when the instance runs as CS |

`ServerTlsPolicy`, tagged `mode`:

| `mode` | Payload | Meaning |
|---|---|---|
| `none` | — | plain listener |
| `tls` | `identity: CertSource` | presents a server certificate, requests none |
| `mutual` | `identity: CertSource`, `verification: CertVerification` | also requests and verifies a client certificate |

`ClientTlsPolicy`, tagged `mode`:

| `mode` | Payload | Meaning |
|---|---|---|
| `none` | — | plain connection |
| `tls` | `verification: CertVerification` | verifies the server, presents no identity |
| `mutual` | `verification: CertVerification`, `identity: CertSource` | also presents a client certificate |

`CertSource`, tagged `source`:

| `source` | Payload | Meaning |
|---|---|---|
| `ephemeral` | — | server only: no material configured, bind an ephemeral self-signed certificate and log the fallback (OC-R-095) |
| `self-signed` | — | ephemeral self-signed pair, explicitly chosen, no fallback logged |
| `files` | `cert_file`, `key_file` — both required | PEM chain and matching private key |

`CertVerification`, tagged `verify`:

| `verify` | Payload | Meaning |
|---|---|---|
| `skip` | — | accept any peer certificate unauthenticated. Test rigs only |
| `root-store` | `extra_ca_files` — list, may be empty | client only: webpki root store plus these anchors |
| `ca-files` | `ca_files` — list, non-empty | exactly these anchors, not the root store |

### 9.1 Derivation rules

- **Basic Auth on** iff *both* `username` and `password` set. Either alone inert.
- **TLS on** for an instance iff its own role's policy is other than `none`. The variant *is* the state; the other role's policy never affects it.
- **mTLS on** iff that policy's `mode` is `mutual`. A `mutual` client always carries an identity, a `mutual` server always a verification — required by the variant.
- **Endpoint scheme gates TLS** in both roles (OC-R-042). `ws://` always plaintext; any policy alongside inert. A URL never advertises a transport its peer does not speak.
- **A `wss://` server's TLS material** follows its `identity` directly (OC-R-096); the old "explicit files always win" precedence is unrepresentable.
- **A `wss://` server with identity `ephemeral`** binds an ephemeral self-signed certificate, not plain TCP, and logs the fallback (OC-R-095).
- Two role-only rejections at construction: `verify = "root-store"` under `tls.server`; `source = "ephemeral"` as a `tls.client` identity.
- **The setup dialog does not expose `protocol` as an input** (OC-R-127): it displays a scheme derived from the TLS selector — `wss://` at TLS/mTLS, `ws://` at Off — and writes the matching `protocol`. The field (§7) is unchanged; a hand-written config may still pair any scheme with any policy, subject to OC-R-042/OC-R-097.

### 9.2 Security profiles

| Profile | Configuration |
|---|---|
| 1 — Basic Auth over `ws://` | `username` + `password`, `protocol = ws` |
| 2 — TLS, server cert only | `protocol = wss`; CSMS `tls.server.mode = "tls"` with any `identity`; CS `tls.client.mode = "tls"` with a `verification`. Optionally plus Basic Auth |
| 3 — mutual TLS | Profile 2 with `mode = "mutual"` on both ends: CSMS adds `verification` (`ca-files`), CS adds `identity` |

```toml
# One OCPP device config, both roles present; the instance's role picks one
[security]
username = "cs001"
password = "hunter2"

# used when this device runs as CSMS: require client certs from a private CA
[security.tls.server]
mode = "mutual"
[security.tls.server.identity]
source = "files"
cert_file = "/etc/ferrowl/csms.crt"
key_file  = "/etc/ferrowl/csms.key"
[security.tls.server.verification]
verify = "ca-files"
ca_files = ["/etc/ferrowl/fleet-ca.pem"]

# used when it runs as CS: platform roots plus one private CA, no identity
[security.tls.client]
mode = "tls"
[security.tls.client.verification]
verify = "root-store"
extra_ca_files = ["/etc/ferrowl/private-ca.pem"]
```

---

## 10. `:` commands

Protocol-specific commands owned here (mechanism owned by `tui/`).

### 10.1 Client (CS) view

| Command | Effect |
|---|---|
| `:start` | connect to the CSMS |
| `:stop` | disconnect |
| `:restart` | disconnect, then reconnect |
| `:e` / `:edit` | open the module setup dialog |
| `:wd` / `:write-device [path]` | save the device config |
| `:compact` | toggle compact table rows |
| `:log [file]` | set (no argument: clear) the persistent log file |

### 10.2 Server (CSMS) view

| Command | Effect |
|---|---|
| `:start` | bind the listener |
| `:stop` | unbind the listener, discard every observed station entry |
| `:restart` | rebind the listener; discards every observed station entry |
| `:e` / `:edit` | open the module setup dialog |
| `:wd` / `:write-device [path]` | save the device config |
| `:compact` | toggle compact table rows |
| `:log [file]` | set (or clear) the persistent log file |

A client does **not** connect on creation — `:start` required. A server binds automatically on creation.
