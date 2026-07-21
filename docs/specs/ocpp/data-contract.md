# OCPP — Data Contract

The OCPP-JSON wire format: the three envelope shapes, message-id semantics,
payload shapes, the typed-dialog vs raw-JSON classification of every action, and
the OCPP state model the simulator maintains.

---

## 1. Transport

OCPP-J over a single WebSocket. Exactly one transport exists — JSON over
WebSocket. There is no OCPP-SOAP, no OCPP over MQTT, and no binary framing.

- Scheme `ws://` (plain) or `wss://` (TLS).
- The WebSocket subprotocol token is fixed by the OCPP version: `ocpp1.6`,
  `ocpp2.0.1`, `ocpp2.1`. The CS advertises it on the upgrade request; the CSMS
  requires it and echoes it back.
- Every OCPP-J envelope travels in a WebSocket **text** frame. Binary, ping, and
  pong frames are not OCPP-J payloads and are ignored.
- The connection is full-duplex: both peers originate Calls on the same socket at
  the same time.

---

## 2. Envelope shapes

Each envelope is a JSON array whose first element is the message-type id.

| Type | Id | Shape |
|---|---|---|
| Call | `2` | `[2, uniqueId, action, payload]` |
| CallResult | `3` | `[3, uniqueId, payload]` |
| CallError | `4` | `[4, uniqueId, errorCode, errorDescription, errorDetails]` |

Element rules:

| Element | Type | Rule |
|---|---|---|
| message-type id | integer | must be exactly 2, 3, or 4 |
| `uniqueId` | string | any string |
| `action` | string | the wire action name (Call only) |
| `payload` | any JSON value | the request (Call) or response (CallResult) body |
| `errorCode` | string | one of the ten fixed codes; anything else reads as `GenericError` |
| `errorDescription` | string | free text |
| `errorDetails` | any JSON value | structured detail; the empty object `{}` when a rejection carries none |

Arity is exact: 4 elements for a Call, 3 for a CallResult, 5 for a CallError. Any
other count is a framing error.

---

## 3. Message-id semantics

- The unique id is the **only** correlation key. A reply carries no action name,
  so the originating Call's action determines which response type its payload is
  decoded as.
- Every **outbound** Call generates a fresh UUID v4 unique id.
- An **inbound** Call's unique id is whatever string the peer chose — no format is
  assumed — and it is echoed back verbatim on the CallResult or CallError.
- An inbound CallResult/CallError whose unique id matches no in-flight outbound
  Call is discarded silently.
- A fire-and-forget Call registers no correlation entry; any reply to it is
  therefore discarded by the rule above.
- Unique ids are not reused, not sequenced, and carry no ordering meaning.

---

## 4. Payload shapes

An action's request and response payloads are exactly the OCPP schema types for
that action and version — carried through untouched, not remapped into any
neutral or simplified representation. There is deliberately **no** version-neutral
semantic layer: the surface is the per-version action set, so every action is
listable and every raw request/response JSON is inspectable.

Consequences that are part of the contract:

- A payload field's name, nesting, and casing are the OCPP spec's, per version. A
  1.6 `connectorId` and a 2.x `evse.id` are different fields, not two spellings of
  one.
- 2.x actions target a connector either through a nested `evse.id` object or,
  for a few (e.g. `TransactionEvent`), through a flat top-level `evseId`. Both are
  recognized as the EVSE target.
- Response payloads are produced by the peer; the simulator's own responses are
  the schema's `Default`-derived value unless the action is one it explicitly
  models (§8).

---

## 5. Typed dialog vs raw JSON

Every action reachable through a send dialog is **exactly one** of *typed* or
*raw JSON* — never both, never neither.

### 5.1 The rule

An action gets a **typed** dialog unless its request's required fields include a
nested object, or a repeated list with no optional escape hatch. Those shapes the
flat property table cannot represent, so they stay on the raw JSON editor.

A typed dialog is a flat property table of `(name, kind, value)` rows:

| Property kind | Editor |
|---|---|
| text | free text |
| number | numeric |
| bool | boolean dropdown |
| enum | dropdown over a closed set of allowed strings |
| timestamp | RFC3339, defaulting to now |

Each row also carries a prefill source — an observed state field, a freshly
generated transaction id, the current time, a fixed constant, or nothing — and an
optional flag. A required-but-empty row is treated as absent.

A typed dialog **always** additionally offers a raw-JSON mode, prefilled from the
current rows. The reverse is not true: a raw-JSON action has no typed mode.

### 5.2 Assemblers

Two typed shapes exist:

- **Flat** — the rows assemble directly into a flat JSON object.
- **Nested** — a small number of flat rows are folded into a full nested request
  by a custom assembler. Used where a nested required shape is nonetheless driven
  by a handful of scalars (e.g. installing a charging profile from a connector id,
  limit, purpose, stack level, and rate unit; pushing a local auth list from a
  version, an update type, and one id tag).

### 5.3 State-driven actions

A subset of CS-originated actions is built entirely from the charging station's
observed state and is sent without any dialog:

| Version | State-driven actions |
|---|---|
| 1.6 (7) | `Authorize`, `BootNotification`, `Heartbeat`, `MeterValues`, `StartTransaction`, `StatusNotification`, `StopTransaction` |
| 2.0.1 / 2.1 (5) | `Authorize`, `BootNotification`, `Heartbeat`, `MeterValues`, `StatusNotification` |

2.x has no `StartTransaction`/`StopTransaction` action; transaction start/stop are
shortcuts that build a `TransactionEvent` for the targeted connector.

### 5.4 Raw-JSON actions

| Version | Raw-JSON actions |
|---|---|
| 1.6 (1) | `GetConfiguration` — a key list; sent directly (empty = all keys) and never opens a form |
| 2.0.1 (8) | `SetNetworkProfile`, `SetVariableMonitoring`, `NotifyEVChargingNeeds`, `NotifyEVChargingSchedule`, `NotifyMonitoringReport`, `NotifyReport`, `ReportChargingProfiles`, `TransactionEvent` |
| 2.1 (16) | the 8 above, plus `BatterySwap`, `GetCertificateChainStatus`, `OpenPeriodicEventStream`, `ReportDERControl`, `AdjustPeriodicEventStream`, `ChangeTransactionTariff`, `SetDefaultTariff`, `UpdateDynamicSchedule` |

Every raw-JSON action ships a template payload that decodes and validates against
its own version's request type. 2.1 overrides one shared template
(`NotifyMonitoringReport`), whose 2.1 schema requires an extra field.

Everything else — including all 26 of 2.1's new actions not listed above — has a
typed dialog. Among 2.1's new actions, `RequestBatterySwap` and
`NotifyAllowedEnergyTransfer` use the nested-assembler form; the rest are flat.

---

## 6. Charging Station state model (client role)

State is split by level and is shared between the view, the inbound handler, and
the Lua sim thread.

### 6.1 Charge-point level

| Field | Notes |
|---|---|
| model, vendor, firmware version, serial number | boot identity, sent in `BootNotification` |
| ICCID, IMSI, meter serial number, meter type (1.6 only) | optional identity fields, sent in `BootNotification` only when non-empty; no 2.x equivalent |
| configuration / variable store | list of `(key, value, readonly)`; answers `GetConfiguration` (1.6) / `GetVariables` (2.x) and is mutated by `ChangeConfiguration` / `SetVariables`. Seeded from the device config, or from the version's built-in defaults when that is empty |
| heartbeat interval | seconds, taken from the CSMS's `BootNotification` response. Unset until a boot round-trips |
| reservation | id tag + reservation id of a charge-point-wide reservation |
| connectors | one or more connector states |

### 6.2 Connector level

| Field | Notes |
|---|---|
| connector id (1.6) / EVSE id + connector id (2.x) | addressing |
| phases, voltage, 3× current, power, frequency | metering, fed into `MeterValues` |
| total energy, session energy, state of charge, temperature | metering |
| status | the version's connector/EVSE status enum |
| RFID tag | the id tag this connector presents |
| transaction | 1.6: an integer id assigned by the CSMS. 2.x: a locally minted string id plus a sequence counter and a *confirmed* flag |
| charging limits | one per charging-profile purpose (transaction, default, maximum, and — 2.x only — external constraints), each with its own rate unit |
| reservation | id tag + reservation id of a connector-level reservation |

### 6.3 Level semantics

- In 1.6, connector id `0` addresses the charge point itself. In 2.x, an absent
  EVSE target (or EVSE id `0`) addresses the charging station itself.
- An inbound Call carrying a top-level connector/EVSE id the station does not have
  is rejected; `0` and an absent id are always valid.
- 2.x auto-`MeterValues` transmit only once the CSMS has **confirmed** the
  transaction start, so a failed start never leaks meter readings. 1.6 transmits
  as soon as a transaction id exists.
- Ending a transaction clears the transaction-scoped charging limit only; the
  default and maximum limits persist.

---

## 7. CSMS state model (server role)

The CSMS is not configured with a station topology; it **observes** one.

| Element | Notes |
|---|---|
| connection id | opaque `u64`, assigned monotonically from 1, one per accepted socket |
| charge-point identity | the last non-empty path segment of the upgrade URL, kept as metadata against a connection id — never as the key, so reconnects and duplicate identities do not collide |
| station entries | one per connected station, discovered from its inbound traffic; connectors/EVSEs under it are discovered the same way |
| per-entry message log | each station/connector entry keeps its own log; there is no single shared log |
| RFID accept-lists | one charge-point-wide list plus one per connector/EVSE scope |
| transaction ids | minted by the CSMS, unique per server instance, monotonic from 1 |

Its observed state is transient: it is discarded on `:stop` and `:restart`, and
the CSMS's configuration is never persisted to the device config (only the RFID
accept-lists are).

### 7.1 RFID accept-list semantics

An entry's **effective set** is its own list unioned with the charge-point-wide
list.

- An **empty** effective set accepts every tag (open mode).
- A **non-empty** effective set accepts only the tags it lists.
- Authorization, which names no connector, is checked against the charge-point-wide
  list unioned with **every** connector list.
- A transaction start, which names a connector, is checked against **that
  connector's** effective set only — a tag listed on one connector does not
  authorize another connector.

---

## 8. Simulated responses

The CSMS answers CS-originated Calls with the action's `Default`-derived response,
except for four it crafts:

| Action | Crafted response |
|---|---|
| boot notification | accepted, with the current time and a heartbeat interval |
| heartbeat | the current time |
| authorization | an accept/reject status, gated by the RFID accept-lists |
| transaction start | a freshly minted unique transaction id plus an accept/reject status |

The CS answers CSMS-originated Calls from its own state where it models them
(configuration read/write, reset, availability, reservations, remote start/stop,
charging-profile set/clear, unlock), and default-accepts everything else with the
action's `Default`-derived response.

---

## 9. Message log

Every request/response pair in either direction is recorded as a message with a
monotonic sequence number, a timestamp, a direction, an action name, the raw
payload, a success/error/neutral outcome, a context string, and the charge-point
or connector scope it belongs to.

- The in-memory buffer holds the most recent **200** messages, evicting
  oldest-first.
- Messages are teed to the persistent log file (when configured) by sequence
  number, so eviction never loses a line.
- The displayed log is filtered to the selected connector/charge-point scope; the
  persistent log carries every scope.
