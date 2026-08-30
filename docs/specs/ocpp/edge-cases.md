# OCPP — Edge Cases and Known Limitations

Boundary behavior, error semantics, intentional constraints. §6 is working as implemented; recorded so it is not "fixed".

---

## 1. Framing boundaries

| Condition | Behavior |
|---|---|
| Frame not valid JSON | framing error: logged, skipped, **connection stays up**. No id to answer |
| Valid JSON, not an array | framing error: logged, skipped. No id to answer |
| Message-type id not 2, 3, or 4 (or not an integer) | framing error: logged, skipped. No id to answer |
| Malformed **Call** (type 2) with string `uniqueId` — wrong arity, or non-string `action` | logged, answered CallError `FormationViolation` with the recovered id. Peer never left to time out |
| Malformed Call, `uniqueId` not a string | logged, skipped — nothing to address a CallError to |
| Malformed **CallResult** or **CallError** (types 3, 4) | logged, skipped — never answered, even with a readable id. A CallError about a CallError is not a valid exchange; a CallResult has no pending call on the peer to fail |
| Extra elements beyond the expected arity | rejected — arity exact, not a minimum |
| Unrecognized `errorCode` on an inbound CallError | accepted, read as `GenericError`. No error |
| Binary / ping / pong frame | ignored; not OCPP-J |
| WebSocket close frame | ends the connection cleanly |
| Transport error while reading | logged; ends the connection |

---

## 2. Call and reply boundaries

| Condition | Behavior |
|---|---|
| Inbound Call names an action the negotiated version lacks | CallError `NotImplemented`; connection up |
| Inbound Call payload fails to deserialize | CallError `FormationViolation` |
| Inbound Call payload fails the version's validation rules | CallError `FormationViolation` |
| Handler rejects an inbound Call | CallError with the handler's code, description, details; connection up |
| Handler's response fails to encode | CallError `FormationViolation` (serialization failure) |
| Awaited outbound Call exceeds the reply timeout | entry discarded; caller gets `GenericError` ("call timed out"). A later reply is discarded |
| Awaited outbound Call whose action fails to encode | caller gets the encoding failure as a rejection; nothing sent |
| Outbound Call on a closed connection | caller gets `GenericError` ("connection closed") |
| Connection torn down with Calls in flight | **every** pending caller failed with `GenericError` ("connection terminated") |
| Inbound CallResult/CallError for an unknown id | discarded silently |
| Inbound reply for a fire-and-forget Call | discarded silently (no entry registered) |
| Two replies for one id | first completes and removes the entry; second discarded |
| Slow or blocking inbound handler | own task; cannot stall the read pump or delay other Calls |
| Outbound frame channel full (64 pending) | sender waits; never silently dropped |
| Command channel full (32 pending) | sender waits |

---

## 3. Connection-drop boundaries

| Condition | Behavior |
|---|---|
| Connection drops mid-call (CS or CSMS) | reader ends, role's command loop signalled, connection torn down, every pending Call failed, disconnect hook fires |
| Connection drops on the CS | module goes offline, auto-Heartbeat and auto-MeterValues halt, heartbeat counter resets, warning logged; reconnect per §6.1 |
| Connection drops on the CSMS | that connection deregistered; accept loop and other connections unaffected |
| CS socket dropped without explicit `:stop`, then `:start` | stale handle torn down first, then a fresh dial — `:start` is not a silent no-op |
| `:start` on an already-connected CS | no-op |
| CSMS listener fails to bind | logged as an error; retry per §6.2 |
| `accept()` itself errors | logged; accept loop keeps running |

---

## 4. Security boundaries

| Condition | Behavior |
|---|---|
| CSMS has Basic Auth, request has no `Authorization` header | HTTP **401**, handshake refused; expected credential never disclosed |
| CSMS has Basic Auth, header mismatch | HTTP **401** |
| CSMS has no Basic Auth, request sends one | accepted — header ignored |
| Request lacks the version's subprotocol token | HTTP **400**, handshake refused |
| TLS handshake fails on an accepted socket | logged with peer address; socket dropped. Listener keeps accepting |
| CS connects to a TLS CSMS whose certificate is not trusted | dial fails; module reports a connect failure |
| Dialog client-side Root Store toggle and CA list after Skip-Verify toggled On | both hidden and excluded regardless of state, resolving to `CertVerification::Skip` (OC-R-111) |
| Dialog client-role Skip-Verify toggle while TLS selector is Off | hidden entirely, not merely inert — a plain or Basic-Auth-only connection has no certificate to verify (OC-R-111) |
| CSMS `ServerTlsPolicy::Mutual`, `verification` resolves to `CaFiles` with zero `ca_files` | fails at construction/`resolve()`, never at listener start — same tier as `CertSource`'s construction checks (OC-R-039) |
| CSMS `ServerTlsPolicy::Mutual` with self-signed certificate (`identity: CertSource::SelfSigned`) and `verification: CaFiles` with ≥1 file | permitted — self-signed identity and the CAs trusted for client certs are independent (OC-R-040) |
| CSMS `ServerTlsPolicy::Mutual` with self-signed certificate, `CaFiles` with zero files | fails at construction, as any zero-file `CaFiles` — self-signed identity does not exempt it (OC-R-039) |
| Dialog server-role Self-Signed toggle (shown at TLS/mTLS) toggled On after `cert_file`/`key_file` text entered | resolved identity `CertSource::SelfSigned`, both files excluded; stored text preserved for Off (OC-R-110) |
| Dialog TLS selector moved to Off after certificate paths and CA entries entered | resolved policy `None`, no payload; every hidden widget keeps its state; displayed protocol reverts to `ws://` (OC-R-127) |
| Dialog Basic Authentication On, TLS selector Off | accepted — Profile 1, credentials over plain `ws://`; independent axes (OC-R-128) |
| Hand-written `ws://` instance whose own-role policy is not `None`, reopened in the dialog | selector shows TLS or mTLS, Protocol display shows derived `wss://`; confirming writes `wss://` back, promoting the inert pairing into a live one. Dialog derives scheme from policy (OC-R-127), normalising the mismatch; the instance stays inert (OC-R-042/OC-R-097) only while unedited |
| Configured PEM file cannot be opened, or contains no certificate or no private key | CS dial / CSMS bind fails with a TLS error, before socket work |
| `username` without `password` (or vice versa) | Basic Auth **not** enabled; field inert |
| `wss://` **server** endpoint, identity `CertSource::Ephemeral` | binds with an ephemeral self-signed certificate, logs the fallback. Never silently plain TCP |
| `ws://` **client** endpoint with TLS material | material inert; connection plain |
| `ws://` CS endpoint whose `ClientTlsPolicy` would fail validation (empty `ca_files`, `Ephemeral` identity, unreadable PEM) | connects in plaintext; policy neither validated nor loaded — scheme decides transport, material inert (OC-R-097), symmetric with the CSMS scheme gate |
| `ws://` **server** endpoint with `ServerTlsPolicy` other than `None` | material inert; listener plain TCP — symmetric with the `ws://` client. A URL never advertises a transport its listener does not speak |
| `ServerTlsPolicy::Mutual` client certificate signed by any one of several `ca_files` | accepted — "any one matches"; `ca_files` is a trust-anchor set, not an ordered chain (OC-R-039) |
| `ServerTlsPolicy::Mutual` with `CertVerification::Skip`, no client certificate presented | handshake still fails — `Skip` skips the CA/identity check on a *presented* cert, does not make presenting optional (OC-R-039) |
| Dialog server-role Skip Verify toggled On while mTLS selected | shared CA list hidden, resolved verification `CertVerification::Skip` regardless of entries; list preserved, restored when Off (OC-R-113) |
| Dialog client-role (CS) Self Signed toggled On while mTLS selected | Client Cert/Key inputs hidden, resolved identity `CertSource::SelfSigned` regardless of text; text preserved, restored when Off (OC-R-116) |
| Self-signed pair, once generated for a module instance | cached and reused across every bind/connect/reconnect and `:restart`/`:reload`, including a config edit leaving the source self-signed; regenerated only on a transition *into* self-signed; a fresh instance (not `:restart`/`:reload`) discards the cache — pair never on disk (OC-R-037/OC-R-115) |
| CS `CertVerification::CaFiles` with empty `ca_files` | refused at construction and dialog submit (OC-R-036/OC-R-125), same reasoning as the Modbus client role |
| `CertVerification::RootStore` on a CSMS's client-certificate verification | rejected at construction, no toggle offered (OC-R-039) — reasoning in the Modbus edge-cases entry; the CSMS listener shares it |
| `extra_headers` entry whose `name` collides (case-insensitively) with a client-controlled header | construction fails, naming the header (OC-R-117) |
| `extra_headers` entry whose `name` or `value` has a byte outside the allowed grammar (e.g. CR/LF) | construction fails, naming header and field (OC-R-118) |
| `extra_headers` on a server-role device config | inert — client-only |

---

## 5. Simulator boundaries

| Condition | Behavior |
|---|---|
| Inbound Call targets a connector/EVSE the station lacks | CallError `PropertyConstraintViolation` ("unknown connectorId"); connection up |
| Inbound Call targets id `0`, or names none | always valid — the charge point itself |
| Inbound Call the CS simulator does not model | **default-accepted** with the `Default`-derived response |
| Charging profile stack level exceeds the configured maximum | rejected, nothing applied. Absent that key, no ceiling |
| Charging profile with no limit in its schedule | accepted; no limit applied |
| Clear-charging-profile with an unrecognized purpose | clears nothing, still succeeds |
| Clear-charging-profile with no purpose criterion | clears **every** per-purpose limit |
| Cancel-reservation whose id matches nothing | succeeds; nothing cleared |
| Remote start with no connector target | falls back to the **first** connector |
| Remote stop for a transaction id not live | succeeds; nothing stopped |
| Configuration write to a read-only key | rejected |
| Configuration write to an unknown key | **creates** it as writable |
| Configuration read naming unknown keys | known ones returned; unknown listed as unknown |
| BootNotification response with interval `0` or none | treated as unset: 30 s heartbeat |
| Heartbeat interval below 1 s | clamped to 1 s |
| RFID accept-lists all empty | every tag accepted (open mode) |
| Tag listed only on connector A, presented at connector B | rejected at B — not inherited sideways. But it **does** authorize a connector-less authorization request, which unions every list |
| Message buffer exceeds 200 messages | oldest evicted. Messages teed to the log file each refresh tick, so an evicted message is still logged if it survived until the next tick (§6.10) |
| 1.6 ICCID/IMSI/meter serial/meter type left empty | omitted from `BootNotification` entirely, not sent as an empty string — wire field requires length ≥ 1 when present |

---

## 6. Known limitations — intentional constraints

### 6.1 CS reconnect resets on handshake, not on message exchange

A CS reconnects on a failed dial or dropped connection with the shared bounded-exponential-backoff driver (MB-R-051; OC-R-048, OC-R-105–107), governed by `reconnect` (default enabled). No queueing of commands issued while disconnected — a non-terminate command sent while backing off is dropped, as Modbus (MB-R-054).

Nuance: backoff resets to 1 s as soon as the WebSocket handshake completes, before any OCPP message (OC-R-105). A peer that accepts the socket and immediately drops it, every time, sees the backoff reset on every attempt — retrying near 1 s — rather than growing as it would if reset required a message exchange.

Consequence: a dropped CS module recovers on its own; auto-Heartbeat and auto-MeterValues resume once reconnected.

### 6.2 CSMS bind retry

A CSMS whose bind fails retries with the shared backoff driver (OC-R-083, OC-R-108–109). A server still binds **automatically on creation**, unlike a client. Governed by the same `reconnect` field as the CS role (default enabled); disabled, a failed bind ends the module task, mirroring Modbus (MB-R-130–134).

### 6.3 Unbounded connections and no idle timeout

A CSMS serves any number of concurrent connections, no cap, no idle timeout. A station that connects and goes silent holds its connection and registry entry indefinitely.

### 6.4 No version-neutral semantic layer

Deliberately no neutral abstraction over the three versions. The surface is the per-version action set, so every action can be listed and every raw payload inspected. A version-neutral layer existed and was removed on purpose; sharing between 2.0.1 and 2.1 is plain shared functions.

Consequence: adding a version means adding its action table, inbound handlers, and action-spec module. No single seam makes it free.

### 6.5 `NotifyPeriodicEventStream` is not an action

OCPP 2.1's `NotifyPeriodicEventStream` is a one-way streaming datagram with no request/response pair, so it cannot be an action-table entry. Intentionally absent from the 90-action set; can be neither sent nor received.

### 6.6 The RFID accept-list is the only CSMS authorization model

The simulated CSMS accepts or rejects a tag purely by list membership. No local auth list, auth cache, expiry, parent id tag, or group-id handling — those actions exist on the wire and are default-accepted, changing no CSMS state.

### 6.7 Server-side configuration is transient

A CSMS's observed state — station entries, connectors, per-station configuration — is discarded on `:stop` and `:restart`, never written to the device config. Only RFID accept-lists persist. The client role persists its connector table and configuration-key store.

### 6.8 Connector count is unbounded

Nothing caps the connectors a client-role device config declares or that may be added at runtime.

### 6.9 A stale reply is silently dropped

A reply arriving after its Call timed out finds no entry and is discarded with no log line. Caller's side: the Call failed; wire's side: the peer answered. Not reconciled.

### 6.10 A message burst larger than the buffer can lose log lines

Messages are teed from memory into the log file once per refresh tick (~100 ms), by sequence number. A message both created **and** evicted between two ticks — more than 200 messages in one tick — is never seen by the tee and does not reach the file. Unreachable at any realistic OCPP rate.

### 6.11 A cancelled inbound Call handler's side effects may have partially applied

Teardown (OC-R-121) aborts an in-flight handler rather than waiting. A side effect already begun (partial state mutation) is not rolled back — only the reply is guaranteed never sent.

### 6.12 The CS and CSMS connection drivers stay separate

`cs::core::run` and `csms::core::run_connection` share a skeleton — build dispatch, start connection, `on_connected`, `select!` loop over commands, `shutdown`, `on_disconnected` — and are deliberately not unified.

Differences thread through the whole body, not the ends. CSMS carries a `ConnectionId` into every handler call (`on_connected(conn)`, `handle_call(conn, action)`, `on_disconnected(conn)`), so `CsActionHandler` and `CsmsActionHandler` have different signatures and no single generic bound covers both without an adapter trait. CS returns `RunEnd::{Terminated, Disconnected}`, which its retry loop classifies to stop or back off; CSMS returns `()` and deregisters from the registry. Command enums differ in name and variants.

Consequence: a change to the duplex loop is made twice. Unifying costs a handler-adapter trait plus a generic over the return type to save ~45 lines across two 90-line files, and would obscure the retry-classification contract `RunEnd` makes explicit.

`wait_backoff` in `ferrowl-util` is already shared: a utility over a channel and a clock, not a lifecycle abstraction spanning the roles.
