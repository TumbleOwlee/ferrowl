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
| CS has `insecure_skip_verify` set | any server certificate is accepted; `ca_file` is **ignored**, not combined. The channel is still encrypted and signatures are still verified — only the certificate identity check is skipped |
| CS is configured with a client certificate but no key (or vice versa) | no client certificate is presented; the connection proceeds without mTLS rather than failing |
| CSMS has `require_client_cert` but no `client_ca_file` | the listener **fails to start** |
| CSMS has `require_client_cert` and a self-signed certificate | the listener **fails to start** — there is no CA to distribute for mTLS in that mode |
| A configured PEM file cannot be opened, contains no certificate, or contains no private key | the CS dial / CSMS bind fails with a TLS error, before any socket work |
| `username` set without `password` (or vice versa) | Basic Auth is **not** enabled; the field is inert |
| A `wss://` **server** endpoint with no TLS material configured at all | binds with an ephemeral self-signed certificate and logs the fallback. It never silently binds plain TCP |
| A `ws://` **client** endpoint with TLS material configured | the TLS material is inert; the connection is plain |
| A `ws://` **server** endpoint with server certificate files configured | the TLS material is inert; the listener is plain TCP — symmetric with the `ws://` client above. The scheme decides the transport, so a URL never advertises a transport its listener does not speak |

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

### 6.1 No auto-reconnect

The OCPP crate has **no** reconnect logic at all, in either role. A CS whose
connection drops — for any reason: peer close, transport error, timeout on the
socket — stays disconnected until an operator issues `:start` or `:restart`. There
is no backoff, no retry loop, and no queueing of commands issued while
disconnected.

This is deliberately unlike Modbus, whose client reconnects with a bounded
exponential backoff (see `modbus/requirements.md`, MB-R-050 – MB-R-055). OCPP has
no equivalent. Nothing in the config turns it on.

Consequence: a CS module that was online and dropped shows offline and stays that
way; auto-Heartbeat and auto-MeterValues halt and do not resume on their own.

### 6.2 No listener retry

A CSMS whose bind fails logs the error and gives up; it does not retry the bind on
a later tick. Rebinding is the operator's job (`:start` / `:restart`). A server
does, however, bind **automatically on creation** — unlike a client, which never
connects until told to.

### 6.3 Unbounded connections and no idle timeout

A CSMS accepts and serves any number of concurrent connections, with no cap and no
idle timeout. A charging station that connects and goes silent holds its
connection and its registry entry indefinitely.

### 6.4 A remotely started transaction is invisible to the CSMS

When the CS simulator accepts a remote-start, it mints a **local** transaction id
(one greater than any it already holds) and puts the connector into a charging
state. It does not then send a transaction-start message, so the CSMS never learns
that id and the two sides disagree about the transaction. This is a simulator
simplification, not a protocol behavior.

### 6.5 No version-neutral semantic layer

There is deliberately no neutral abstraction over the three versions. The surface
is the per-version action set, so that every action can be listed and every raw
request/response payload can be inspected. A version-neutral layer existed and was
removed on purpose; the code sharing between 2.0.1 and 2.1 is plain shared
functions, not an abstraction.

Consequence: adding a version means adding its action table, its inbound handlers,
and its action-spec module. There is no single seam that makes it free.

### 6.6 `NotifyPeriodicEventStream` is not an action

OCPP 2.1's `NotifyPeriodicEventStream` is a one-way streaming datagram with no
request/response pair, so it cannot be an entry in an action table. It is
intentionally absent from the 90-action 2.1 set, and can be neither sent nor
received.

### 6.7 The RFID accept-list is the only CSMS authorization model

The simulated CSMS accepts or rejects an id tag purely by list membership. There is
no local auth list, no auth cache, no expiry, no parent id tag, and no group-id
handling — those actions exist on the wire and are default-accepted, but they
change no CSMS state.

### 6.8 Server-side configuration is transient

A CSMS's observed state — station entries, connectors, per-station configuration —
is discarded on `:stop` and `:restart` and is never written to the device config.
Only the RFID accept-lists persist. The client role, by contrast, persists its
connector table and its configuration-key store.

### 6.9 Connector count is unbounded

Nothing caps the number of connectors a client-role device config may declare, or
that may be added at runtime.

### 6.10 A stale reply is silently dropped

A reply that arrives after its Call timed out finds no correlation entry and is
discarded with no log line. From the caller's side the Call simply failed; from the
wire's side the peer did answer. The two views are not reconciled.

### 6.11 A message burst larger than the buffer can lose log lines

Messages are teed from the in-memory buffer into the persistent log file once per
refresh tick (~100 ms), by sequence number. A message that is both created **and**
evicted between two ticks — i.e. more than 200 messages arrive in one tick — is
never seen by the tee and does not reach the log file. This is unreachable at any
realistic OCPP message rate.
