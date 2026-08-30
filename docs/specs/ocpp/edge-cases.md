# OCPP — Edge Cases and Known Limitations

Boundary behavior, error semantics, and the constraints that are **intentional**.
Everything in §6 is working as implemented; it is recorded here so it is not
mistaken for an oversight and silently "fixed".

---

## 1. Framing boundaries

| Condition | Behavior |
|---|---|
| Frame is not valid JSON | framing error: logged, frame skipped, **connection stays up**. No id to answer |
| Frame is valid JSON but not an array | framing error: logged, skipped. No id to answer |
| Message-type id is not 2, 3, or 4 (or is not an integer) | framing error: logged, skipped. No id to answer |
| Malformed **Call** (type 2) whose `uniqueId` is a string — wrong arity, or non-string `action` | framing error: logged, and answered with CallError `FormationViolation` carrying the recovered id. The peer is never left to time out |
| Malformed Call whose `uniqueId` is not a string | logged, skipped — nothing to address a CallError to |
| Malformed **CallResult** or **CallError** (types 3 and 4) | logged, skipped — never answered, even when the id is readable. A CallError about a CallError is not a valid exchange, and a CallResult has no pending call on the peer to fail |
| Extra elements beyond the expected arity | rejected — arity is exact, not a minimum |
| Unrecognized `errorCode` string on an inbound CallError | accepted, read as `GenericError`. No error |
| Binary / ping / pong WebSocket frame | ignored; not treated as OCPP-J |
| WebSocket close frame | ends the connection cleanly |
| Transport error while reading | logged; ends the connection |

---

## 2. Call and reply boundaries

| Condition | Behavior |
|---|---|
| Inbound Call names an action the negotiated version does not have | CallError `NotImplemented`; connection stays up |
| Inbound Call's payload fails to deserialize into the request type | CallError `FormationViolation` |
| Inbound Call's payload fails the version's validation rules | CallError `FormationViolation` |
| Handler rejects an inbound Call | CallError with the handler's code, description, and details; connection stays up |
| Handler's response fails to encode | CallError `FormationViolation` (a serialization failure) |
| Awaited outbound Call exceeds the reply timeout | the correlation entry is discarded; the caller gets a `GenericError` rejection ("call timed out"). A reply arriving later is discarded |
| Awaited outbound Call whose action fails to encode | the caller gets the encoding failure as a rejection; nothing is sent |
| Outbound Call sent on a closed connection | the caller gets a `GenericError` rejection ("connection closed") |
| Connection torn down with Calls in flight | **every** pending caller is failed with a `GenericError` rejection ("connection terminated") — no caller is left hanging |
| Inbound CallResult/CallError for an unknown unique id | discarded silently |
| Inbound reply for a fire-and-forget Call | discarded silently (no correlation entry was registered) |
| Peer sends two replies for one unique id | the first completes the entry and removes it; the second is discarded |
| Slow or blocking inbound handler | runs in its own task; it cannot stall the read pump or delay other Calls |
| Outbound frame channel full (64 pending) | the sender waits; frames are never silently dropped |
| Command channel full (32 pending) | the sender waits |

---

## 3. Connection-drop boundaries

| Condition | Behavior |
|---|---|
| Connection drops mid-call (CS or CSMS) | the reader ends, the role's command loop is signalled, the connection is torn down, every pending Call is failed, and the disconnect hook fires |
| Connection drops on the CS | the module goes offline, auto-Heartbeat and auto-MeterValues halt, the heartbeat counter resets, and a warning is logged. **No reconnect is attempted** (§6.1) |
| Connection drops on the CSMS | that connection is deregistered; the accept loop and every other connection are unaffected |
| A CS module's socket dropped without an explicit `:stop`, then `:start` is issued | the stale handle is torn down first, then a fresh dial is made — `:start` is not a silent no-op |
| `:start` on an already-connected CS | no-op |
| A CSMS listener that fails to bind | logged as an error; the module does not retry (§6.2) |
| `accept()` itself errors | logged; the accept loop keeps running |

---

## 4. Security boundaries

| Condition | Behavior |
|---|---|
| CSMS has Basic Auth configured, request has no `Authorization` header | HTTP **401**, handshake refused; the expected credential is never disclosed |
| CSMS has Basic Auth configured, header does not match | HTTP **401** |
| CSMS has no Basic Auth configured, request sends one anyway | accepted — the header is ignored |
| Request does not advertise the version's subprotocol token | HTTP **400**, handshake refused |
| TLS handshake fails on an accepted socket | logged with the peer address; that socket is dropped. The listener keeps accepting |
| CS connects to a TLS CSMS whose certificate is not trusted | the dial fails; the module reports a connect failure |
| The OCPP setup dialog's client-side Root Store toggle and CA list after Skip-Verify is toggled On | both hidden and excluded from the resolved verification regardless of state already present, resolving to `CertVerification::Skip` (OC-R-111) |
| The OCPP setup dialog's client-role Skip-Verify toggle while the TLS selector is Off | hidden entirely, not merely inert — a plain or Basic-Auth-only connection has no certificate to verify, so the toggle is shown only at TLS/mTLS (OC-R-111) |
| CSMS selects `ServerTlsPolicy::Mutual` but `verification` resolves to `CaFiles` with zero `ca_files` | fails at construction/`resolve()` time, never at listener-start time — the same tier as `CertSource`'s own construction-time checks (OC-R-039) |
| CSMS selects `ServerTlsPolicy::Mutual` with a self-signed certificate (`identity: CertSource::SelfSigned`) and `verification: CaFiles` holding at least one CA file | permitted — the server's self-signed identity and the CA(s) trusted for verifying client certs are independent (OC-R-040) |
| CSMS selects `ServerTlsPolicy::Mutual` with a self-signed certificate and `verification` resolves to `CaFiles` with zero CA files | fails at construction, same as any `CaFiles` with zero `ca_files` — the self-signed identity does not exempt it (OC-R-039) |
| The OCPP setup dialog's Self-Signed toggle (server role, shown while the TLS selector is TLS or mTLS) toggled On after `cert_file`/`key_file` text was entered | resolved identity becomes `CertSource::SelfSigned`, excluding both files; the widgets' stored text is preserved for when the toggle goes back Off (OC-R-110) |
| The OCPP setup dialog's TLS selector moved to Off after certificate paths and CA entries were entered | the resolved policy is `None` and carries no payload at all; every hidden widget keeps its stored state and the displayed protocol reverts to `ws://` (OC-R-127) |
| The OCPP setup dialog with Basic Authentication On and the TLS selector Off | accepted — Profile 1, credentials over plain `ws://`; the two selections are independent axes and neither constrains the other (OC-R-128) |
| A configured PEM file cannot be opened, contains no certificate, or contains no private key | the CS dial / CSMS bind fails with a TLS error, before any socket work |
| `username` set without `password` (or vice versa) | Basic Auth is **not** enabled; the field is inert |
| A `wss://` **server** endpoint whose identity is `CertSource::Ephemeral` | binds with an ephemeral self-signed certificate and logs the fallback. It never silently binds plain TCP |
| A `ws://` **client** endpoint with TLS material configured | the TLS material is inert; the connection is plain |
| A `ws://` **server** endpoint with a `ServerTlsPolicy` other than `None` configured | the TLS material is inert; the listener is plain TCP — symmetric with the `ws://` client above. The scheme decides the transport, so a URL never advertises a transport its listener does not speak |
| A `ServerTlsPolicy::Mutual` client certificate signed by any one of several configured `ca_files` | accepted — verification is "any one matches", not "all must match"; `ca_files` is a trust-anchor set, not an ordered chain (OC-R-039) |
| `ServerTlsPolicy::Mutual` with `CertVerification::Skip` and a connection presenting no client certificate at all | handshake still fails — `Skip` skips the CA/identity check on a *presented* cert, it does not make presenting one optional (OC-R-039) |
| The OCPP setup dialog's server-role Skip Verify toggled On while mTLS is selected | the shared CA list widget is hidden and the resolved verification becomes `CertVerification::Skip` regardless of entries already present; list contents preserved, restored when the toggle goes back Off (OC-R-113) |
| The OCPP setup dialog's client-role (CS) Self Signed toggled On while mTLS is selected | the Client Cert/Key inputs are hidden and the resolved identity becomes `CertSource::SelfSigned` regardless of text already present; text preserved, restored when the toggle goes back Off (OC-R-116) |
| A self-signed certificate/key pair, once generated for a module instance | cached and reused across every subsequent bind/connect/reconnect and `:restart`/`:reload`, including a config edit that leaves the resolved source self-signed; regenerated only on a transition *into* self-signed from something else; a fresh module instance (not a `:restart`/`:reload` of the same one) discards the cache instead of reusing it, since the pair is never written to disk (OC-R-037/OC-R-115) |
| A CS `CertVerification::CaFiles` with an empty `ca_files` | Refused at construction and at dialog submit (OC-R-036/OC-R-125), same reasoning as the Modbus client role |
| `CertVerification::RootStore` on a CSMS's client-certificate verification | Rejected at construction, no toggle offered (OC-R-039) — see the Modbus edge-cases entry for the full reasoning; the CSMS listener shares it |
| `extra_headers` entry whose `name` collides (case-insensitively) with a header the client already controls | construction fails, naming the offending header (OC-R-117) |
| `extra_headers` entry whose `name` or `value` contains a byte outside the allowed grammar (e.g. embedded CR/LF) | construction fails, naming the offending header and field (OC-R-118) |
| `extra_headers` configured on a server-role device config | inert — the field is client-only and has no effect for a CSMS |

---

## 5. Simulator boundaries

| Condition | Behavior |
|---|---|
| Inbound Call targets a connector/EVSE the station does not have | CallError `PropertyConstraintViolation` ("unknown connectorId"); connection stays up |
| Inbound Call targets connector/EVSE id `0`, or names none | always valid — it addresses the charge point itself |
| Inbound Call the CS simulator does not model | **default-accepted** with the action's `Default`-derived response — not rejected |
| Charging profile whose stack level exceeds the configured maximum | rejected, and nothing is applied. Absent that configuration key, no ceiling is enforced |
| Charging profile with no limit in its schedule | accepted; no limit is applied |
| Clear-charging-profile with an unrecognized purpose | clears nothing, and still succeeds |
| Clear-charging-profile with no purpose criterion | clears **every** per-purpose limit |
| Cancel-reservation whose reservation id matches nothing | succeeds; nothing is cleared |
| Remote start with no connector target | falls back to the **first** connector |
| Remote stop for a transaction id that is not live | succeeds; nothing is stopped |
| Configuration write to a read-only key | rejected |
| Configuration write to an unknown key | **creates** the key as writable |
| Configuration read naming unknown keys | the known ones are returned; the unknown ones are listed as unknown |
| BootNotification response with interval `0` or no interval | treated as unset: the CS falls back to a 30 s heartbeat |
| Heartbeat interval below 1 s | clamped to 1 s |
| RFID accept-lists all empty | every tag is accepted (open mode) |
| A tag listed only on connector A, presented at connector B | rejected at B — connector lists are not inherited sideways. But it **does** authorize a connector-less authorization request, which unions every list |
| Message buffer exceeds 200 messages | the oldest are evicted from memory. Messages are teed to the persistent log file on each refresh tick, so an evicted message is still logged provided it survived until the next tick (see §6.11) |
| 1.6 ICCID/IMSI/meter serial/meter type left empty | field omitted from `BootNotification` entirely, not sent as an empty string — the wire field requires length ≥ 1 when present |

---

## 6. Known limitations — intentional constraints

### 6.1 CS reconnect resets on handshake, not on message exchange

Resolved: a CS now reconnects on a failed dial or a dropped connection, using the
same shared bounded-exponential-backoff driver as Modbus's client (see
`modbus/requirements.md`, MB-R-051; `requirements.md`, OC-R-048, OC-R-105–107).
Governed by the `reconnect` config field, default enabled. There is still no
queueing of commands issued while disconnected — a command other than terminate
sent while backing off is dropped, same as Modbus (MB-R-054).

Retained nuance: the backoff resets to 1 s as soon as the WebSocket handshake
completes, before any OCPP message is exchanged (OC-R-105). A peer that accepts
the socket and then immediately drops it, every time, will still see the backoff
reset — and effectively retry near 1 s — on every attempt, rather than the
interval growing as it would if reset required a successful message exchange.

Consequence: a CS module that was online and dropped now recovers on its own;
auto-Heartbeat and auto-MeterValues resume once reconnected without operator
intervention.

### 6.2 CSMS bind retry

Resolved: a CSMS whose bind fails now retries using the same shared backoff
driver (OC-R-083, OC-R-108–109), instead of giving up permanently. A server still
binds **automatically on creation** — unlike a client, which never connects until
told to. Governed by the same `reconnect` config field as the CS role
(default enabled); with it disabled, a failed bind ends the module task
instead of retrying, mirroring Modbus's own server-side toggle
(MB-R-130–134).

### 6.3 Unbounded connections and no idle timeout

A CSMS accepts and serves any number of concurrent connections, with no cap and no
idle timeout. A charging station that connects and goes silent holds its
connection and its registry entry indefinitely.

### 6.4 No version-neutral semantic layer

There is deliberately no neutral abstraction over the three versions. The surface
is the per-version action set, so that every action can be listed and every raw
request/response payload can be inspected. A version-neutral layer existed and was
removed on purpose; the code sharing between 2.0.1 and 2.1 is plain shared
functions, not an abstraction.

Consequence: adding a version means adding its action table, its inbound handlers,
and its action-spec module. There is no single seam that makes it free.

### 6.5 `NotifyPeriodicEventStream` is not an action

OCPP 2.1's `NotifyPeriodicEventStream` is a one-way streaming datagram with no
request/response pair, so it cannot be an entry in an action table. It is
intentionally absent from the 90-action 2.1 set, and can be neither sent nor
received.

### 6.6 The RFID accept-list is the only CSMS authorization model

The simulated CSMS accepts or rejects an id tag purely by list membership. There is
no local auth list, no auth cache, no expiry, no parent id tag, and no group-id
handling — those actions exist on the wire and are default-accepted, but they
change no CSMS state.

### 6.7 Server-side configuration is transient

A CSMS's observed state — station entries, connectors, per-station configuration —
is discarded on `:stop` and `:restart` and is never written to the device config.
Only the RFID accept-lists persist. The client role, by contrast, persists its
connector table and its configuration-key store.

### 6.8 Connector count is unbounded

Nothing caps the number of connectors a client-role device config may declare, or
that may be added at runtime.

### 6.9 A stale reply is silently dropped

A reply that arrives after its Call timed out finds no correlation entry and is
discarded with no log line. From the caller's side the Call simply failed; from the
wire's side the peer did answer. The two views are not reconciled.

### 6.10 A message burst larger than the buffer can lose log lines

Messages are teed from the in-memory buffer into the persistent log file once per
refresh tick (~100 ms), by sequence number. A message that is both created **and**
evicted between two ticks — i.e. more than 200 messages arrive in one tick — is
never seen by the tee and does not reach the log file. This is unreachable at any
realistic OCPP message rate.

### 6.11 A cancelled inbound Call handler's side effects may have partially applied

Teardown (OC-R-121) aborts an in-flight inbound Call handler task rather than
waiting for it. If the handler had already begun a side effect (e.g. a partial
state mutation) before cancellation, that partial effect is not rolled back —
only the reply is guaranteed never to be sent.

### 6.12 The CS and CSMS connection drivers stay separate

`cs::core::run` and `csms::core::run_connection` share a skeleton — build the
dispatch, start the connection, `on_connected`, a `select!` loop over commands,
`shutdown`, `on_disconnected` — and are deliberately not unified.

The differences are threaded through the whole body rather than isolated at the
ends. The CSMS side carries a `ConnectionId` and passes it to every handler call
(`on_connected(conn)`, `handle_call(conn, action)`, `on_disconnected(conn)`), so
`CsActionHandler` and `CsmsActionHandler` have different method signatures and no
single generic bound covers both without a further adapter trait. The CS side
returns `RunEnd::{Terminated, Disconnected}`, which its caller's retry loop
classifies to choose between stopping and backing off; the CSMS side returns `()`
and deregisters from the connection registry instead. Their command enums differ
in name and variants.

Consequence: a change to the duplex loop must be made twice. That is the accepted
price — unifying costs a handler-adapter trait plus a generic over the return
type to save roughly 45 lines across two 90-line files, and would obscure the
retry-classification contract `RunEnd` exists to make explicit.

The shared `wait_backoff` helper in `ferrowl-util` is a different case and is
already shared: it is a utility over a channel and a clock, not a lifecycle
abstraction spanning the two roles.
