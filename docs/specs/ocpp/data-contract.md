# OCPP — Data Contract

OCPP-JSON wire format: three envelopes, message-id semantics, payload shapes, typed-dialog vs raw-JSON classification, simulator state model.

---

## 1. Transport

OCPP-J over a single WebSocket. The only transport — no OCPP-SOAP, no MQTT, no binary framing.

- Scheme `ws://` (plain) or `wss://` (TLS).
- Subprotocol token fixed by version: `ocpp1.6`, `ocpp2.0.1`, `ocpp2.1`. CS advertises on upgrade; CSMS requires it and echoes it back.
- Every envelope travels in a WebSocket **text** frame. Binary, ping, pong are not OCPP-J and are ignored.
- Full-duplex: both peers originate Calls on the same socket concurrently.

---

## 2. Envelope shapes

JSON array, first element = message-type id.

| Type | Id | Shape |
|---|---|---|
| Call | `2` | `[2, uniqueId, action, payload]` |
| CallResult | `3` | `[3, uniqueId, payload]` |
| CallError | `4` | `[4, uniqueId, errorCode, errorDescription, errorDetails]` |

| Element | Type | Rule |
|---|---|---|
| message-type id | integer | exactly 2, 3, or 4 |
| `uniqueId` | string | any string |
| `action` | string | wire action name (Call only) |
| `payload` | any JSON value | request (Call) or response (CallResult) body |
| `errorCode` | string | one of ten fixed codes; anything else reads as `GenericError` |
| `errorDescription` | string | free text |
| `errorDetails` | any JSON value | structured detail; `{}` when a rejection carries none |

Arity exact: 4 for Call, 3 for CallResult, 5 for CallError. Any other count is a framing error.

---

## 3. Message-id semantics

- The unique id is the **only** correlation key. A reply carries no action name; the originating Call's action selects the response type.
- Every **outbound** Call generates a fresh UUID v4.
- An **inbound** Call's id is whatever the peer chose — no format assumed — echoed back verbatim on the CallResult or CallError.
- An inbound CallResult/CallError matching no in-flight outbound Call is discarded silently.
- A fire-and-forget Call registers no entry; any reply is discarded by the rule above.
- Unique ids are not reused, not sequenced, carry no ordering meaning.

---

## 4. Payload shapes

An action's request and response payloads are exactly the OCPP schema types for that action and version — carried through untouched, not remapped. Deliberately **no** version-neutral semantic layer: the surface is the per-version action set, so every action is listable and every raw JSON inspectable.

- A field's name, nesting, and casing are the OCPP spec's, per version. A 1.6 `connectorId` and a 2.x `evse.id` are different fields, not two spellings.
- 2.x actions target a connector through a nested `evse.id` object or, for a few (e.g. `TransactionEvent`), a flat top-level `evseId`. Both recognized as the EVSE target.
- Response payloads are produced by the peer; the simulator's own responses are the schema's `Default`-derived value unless explicitly modelled (§8).

---

## 5. Typed dialog vs raw JSON

Every action reachable through a send dialog is **exactly one** of *typed* or *raw JSON*.

### 5.1 The rule

An action gets a **typed** dialog unless its request's required fields include a nested object, or a repeated list with no optional escape hatch — shapes the flat property table cannot represent; those stay on the raw JSON editor.

A typed dialog is a flat property table of `(name, kind, value)` rows:

| Property kind | Editor |
|---|---|
| text | free text |
| number | numeric |
| bool | boolean dropdown |
| enum | dropdown over a closed set |
| timestamp | RFC3339, defaulting to now |

Each row also carries a prefill source — an observed state field, a freshly generated transaction id, the current time, a fixed constant, or nothing — and an optional flag. A required-but-empty row is treated as absent.

A typed dialog **always** also offers a raw-JSON mode, prefilled from the current rows. A raw-JSON action has no typed mode.

### 5.2 Assemblers

- **Flat** — rows assemble directly into a flat JSON object.
- **Nested** — a few flat rows folded into a full nested request by a custom assembler. Used where a nested required shape is driven by a handful of scalars (installing a charging profile from connector id, limit, purpose, stack level, rate unit; pushing a local auth list from version, update type, one id tag).

### 5.3 State-driven actions

Built entirely from observed state, sent without a dialog:

| Version | State-driven actions |
|---|---|
| 1.6 (7) | `Authorize`, `BootNotification`, `Heartbeat`, `MeterValues`, `StartTransaction`, `StatusNotification`, `StopTransaction` |
| 2.0.1 / 2.1 (5) | `Authorize`, `BootNotification`, `Heartbeat`, `MeterValues`, `StatusNotification` |

2.x has no `StartTransaction`/`StopTransaction`; transaction start/stop are shortcuts building a `TransactionEvent` for the targeted connector.

### 5.4 Raw-JSON actions

| Version | Raw-JSON actions |
|---|---|
| 1.6 (1) | `GetConfiguration` — a key list; sent directly (empty = all keys), never opens a form |
| 2.0.1 (8) | `SetNetworkProfile`, `SetVariableMonitoring`, `NotifyEVChargingNeeds`, `NotifyEVChargingSchedule`, `NotifyMonitoringReport`, `NotifyReport`, `ReportChargingProfiles`, `TransactionEvent` |
| 2.1 (16) | the 8 above plus `BatterySwap`, `GetCertificateChainStatus`, `OpenPeriodicEventStream`, `ReportDERControl`, `AdjustPeriodicEventStream`, `ChangeTransactionTariff`, `SetDefaultTariff`, `UpdateDynamicSchedule` |

Every raw-JSON action ships a template payload that decodes and validates against its own version's request type. 2.1 overrides one shared template (`NotifyMonitoringReport`), whose 2.1 schema requires an extra field.

Everything else — including all 26 of 2.1's new actions not listed — has a typed dialog. Among 2.1's new actions, `RequestBatterySwap` and `NotifyAllowedEnergyTransfer` use the nested assembler; the rest are flat.

---

## 6. Charging Station state model (client role)

State split by level, shared between the view, the inbound handler, and the Lua sim thread.

### 6.1 Charge-point level

| Field | Notes |
|---|---|
| model, vendor, firmware version, serial number | boot identity, sent in `BootNotification` |
| ICCID, IMSI, meter serial number, meter type (1.6 only) | optional identity, sent in `BootNotification` only when non-empty; no 2.x equivalent |
| configuration / variable store | list of `(key, value, readonly)`; answers `GetConfiguration` (1.6) / `GetVariables` (2.x), mutated by `ChangeConfiguration` / `SetVariables`. Seeded from the device config, or the version's built-in defaults when empty |
| heartbeat interval | seconds, from the CSMS's `BootNotification` response. Unset until a boot round-trips |
| reservation | id tag + reservation id of a charge-point-wide reservation |
| connectors | one or more connector states |

### 6.2 Connector level

| Field | Notes |
|---|---|
| connector id (1.6) / EVSE id + connector id (2.x) | addressing |
| phases, voltage, 3× current, power, frequency | metering, fed into `MeterValues` |
| total energy, session energy, state of charge, temperature | metering |
| status | the version's connector/EVSE status enum |
| RFID tag | id tag this connector presents |
| transaction | 1.6: integer id assigned by the CSMS. 2.x: locally minted string id plus sequence counter and *confirmed* flag |
| charging limits | one per charging-profile purpose (transaction, default, maximum, and — 2.x only — external constraints), each with its own rate unit |
| reservation | id tag + reservation id of a connector-level reservation |

### 6.3 Level semantics

- 1.6: connector id `0` = the charge point. 2.x: absent EVSE target (or EVSE id `0`) = the charging station.
- An inbound Call with a top-level connector/EVSE id the station lacks is rejected; `0` and absent always valid.
- 2.x auto-`MeterValues` transmit only once the CSMS has **confirmed** the transaction start, so a failed start never leaks meter readings. 1.6 transmits as soon as a transaction id exists.
- Ending a transaction clears the transaction-scoped limit only; default and maximum persist.

---

## 7. CSMS state model (server role)

The CSMS is not configured with a station topology; it **observes** one.

| Element | Notes |
|---|---|
| connection id | opaque `u64`, monotonic from 1, one per accepted socket |
| charge-point identity | last non-empty path segment of the upgrade URL, kept as metadata against a connection id — never the key, so reconnects and duplicate identities do not collide |
| station entries | one per connected station, discovered from inbound traffic; connectors/EVSEs discovered the same way |
| per-entry message log | each station/connector entry keeps its own log; no single shared log |
| RFID accept-lists | one charge-point-wide plus one per connector/EVSE scope |
| transaction ids | minted by the CSMS, unique per server instance, monotonic from 1 |

Observed state is transient: discarded on `:stop` and `:restart`, never persisted to the device config (only RFID accept-lists are).

### 7.1 RFID accept-list semantics

Effective set = own list ∪ charge-point-wide list.

- **Empty** effective set accepts every tag (open mode).
- **Non-empty** accepts only listed tags.
- Authorization (names no connector) checked against the charge-point-wide list ∪ **every** connector list.
- Transaction start (names a connector) checked against **that connector's** effective set only — a tag on one connector does not authorize another.

---

## 8. Simulated responses

The CSMS answers CS-originated Calls with the `Default`-derived response, except four it crafts:

| Action | Crafted response |
|---|---|
| boot notification | accepted, current time, heartbeat interval |
| heartbeat | current time |
| authorization | accept/reject status, gated by RFID accept-lists |
| transaction start | freshly minted unique transaction id plus accept/reject status |

The CS answers CSMS-originated Calls from its own state where modelled (configuration read/write, reset, availability, reservations, remote start/stop, charging-profile set/clear, unlock), default-accepting everything else.

---

## 9. Message log

Every request/response pair in either direction is recorded as a message with monotonic sequence number, timestamp, direction, action name, raw payload, success/error/neutral outcome, context string, and charge-point/connector scope.

- In-memory buffer holds the most recent **200**, evicting oldest first.
- Messages teed to the persistent log file (when configured) by sequence number, so eviction never loses a line.
- Displayed log filtered to the selected connector/charge-point scope; persistent log carries every scope.
