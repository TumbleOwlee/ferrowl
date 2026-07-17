# OCPP — Requirements

Normative behavior of the OCPP capability area: the version-generic engine, the
Charging Station (CS) and Charging Station Management System (CSMS) roles, the
OCPP-J framing over WebSocket, call correlation and timeouts, the security
profiles, the simulated CS/CSMS state machines, and the OCPP module
configuration.

IDs are stable and append-only (`OC-R-nnn`). See [`../README.md`](../README.md).

Companion documents: [`api-contract.md`](./api-contract.md) (action tables and
config fields), [`data-contract.md`](./data-contract.md) (wire frames, payload
shapes, state model), [`edge-cases.md`](./edge-cases.md) (boundary and error
behavior, stated limitations).

---

## Versions

**OC-R-001** — Exactly three OCPP versions shall be supported: 1.6, 2.0.1, and
2.1. Every version shall be reachable in both roles.

**OC-R-002** — Each version shall declare its complete action table: exactly 28
actions for 1.6, 64 for 2.0.1, and 90 for 2.1.

**OC-R-003** — Each version's action table shall partition into a CS-originated
set and a CSMS-originated set: the two sets shall be disjoint and together shall
cover every action in the table.

**OC-R-004** — Each version shall declare exactly one WebSocket subprotocol
token: `ocpp1.6`, `ocpp2.0.1`, `ocpp2.1`. The token shall be fixed by the
version and shall never be configurable.

**OC-R-005** — Every CSMS-originated action shall carry a connector scope of
`None` (no connector/EVSE field), `Optional` (the field exists but may be
omitted), or `Required` (the field is mandatory). The scope shall be derived from
the presence and optionality of the request's top-level connector/EVSE target.

**OC-R-006** — Each action shall expose a `Default`-derived request template and
a `Default`-derived response template, retrievable by wire action name. An
unknown name shall yield no template rather than an error.

**OC-R-007** — 2.1 shall be a strict superset of 2.0.1: every 2.0.1 action name
shall also exist in 2.1, and the 26 additional 2.1 actions shall be additive.

**OC-R-008** — A version's own request-validation rules shall be applied on every
inbound Call before it reaches a handler, and shall not be applied to outbound
Calls.

---

## OCPP-J framing

**OC-R-009** — The wire format shall be OCPP-J: JSON arrays carried in WebSocket
**text** frames. Exactly three envelope shapes shall exist: Call
`[2, uniqueId, action, payload]`, CallResult `[3, uniqueId, payload]`, and
CallError `[4, uniqueId, errorCode, errorDescription, errorDetails]`.

**OC-R-010** — Decoding shall reject a frame that is not valid JSON, is not a
JSON array, carries an unknown message-type id, has the wrong element count for
its message type (4 / 3 / 5 respectively), or whose `uniqueId`, `action`, or
`errorCode` element is not a JSON string.

**OC-R-011** — A unique id shall be an arbitrary string. Every outbound Call
shall generate a fresh UUID v4 unique id; an inbound Call's unique id shall be
echoed back verbatim on the reply, whatever its form.

**OC-R-012** — The `errorCode` element shall be one of exactly ten spec-fixed
codes (see [`api-contract.md`](./api-contract.md)). An unrecognized code received
on the wire shall be accepted and mapped to `GenericError` rather than failing
the frame.

**OC-R-013** — Non-text WebSocket frames (binary, ping, pong) shall be ignored
without error and shall not be treated as OCPP-J payloads.

---

## Connection engine

**OC-R-014** — A connection shall be full-duplex: both peers may originate Calls
concurrently on the same socket, and the engine shall be role-agnostic — the CS
and CSMS roles shall share one connection implementation.

**OC-R-015** — Outbound frames shall be serialized through a single writer, so
frames are never interleaved on the wire.

**OC-R-016** — Each inbound Call shall be dispatched in its own task, so a slow
or re-entrant handler shall never block the reading of further frames.

**OC-R-017** — An outbound Call awaiting a reply shall be registered in a
correlation table keyed by its unique id. An inbound CallResult or CallError
shall complete the matching entry.

**OC-R-018** — A CallResult carries no action name; the reply shall therefore be
decoded using the originating action to select the response type.

**OC-R-019** — A CallResult or CallError whose unique id matches no pending entry
shall be discarded silently and shall not disturb the connection.

**OC-R-020** — Every awaited outbound Call shall be bounded by the configured
reply timeout. On expiry the correlation entry shall be discarded and the caller
shall receive a `GenericError` rejection.

**OC-R-021** — A Call may also be sent fire-and-forget, with no correlation entry
and no reply delivered to the caller.

**OC-R-022** — On connection teardown every still-pending outbound Call shall be
failed with a `GenericError` rejection, so no caller is left waiting.

**OC-R-023** — A peer's WebSocket close, or a transport error while reading,
shall end the connection: the role's command loop shall be signalled, the
connection shall be torn down, and the role's disconnect hook shall fire.

**OC-R-024** — A malformed inbound frame shall be logged and shall not tear down
the connection. It is otherwise skipped, except where OC-R-098 requires it to be
answered with a CallError.

---

## Errors

**OC-R-025** — A handler shall be able to reject an inbound Call at the protocol
level by returning a call error (code, description, details). This shall be sent
back as a CallError frame and shall leave the connection intact.

**OC-R-026** — An inbound Call naming an action the negotiated version does not
have shall be answered with CallError `NotImplemented`.

**OC-R-027** — An inbound Call whose payload fails to deserialize into the
action's request type, or fails the version's validation rules, shall be answered
with CallError `FormationViolation`.

**OC-R-028** — A CallError received in reply to an outbound Call shall be
surfaced to the caller as a rejection carrying the peer's code, description, and
details verbatim — it shall not be turned into a connection failure.

**OC-R-098** — A frame that fails to decode but is identifiable as a Call
(`messageTypeId` 2) whose `uniqueId` is a string shall be answered with a
CallError carrying that id and the code `FormationViolation`. A peer that sent a
recoverable Call shall never be left to wait out its own call timeout.

**OC-R-099** — A frame whose id cannot be recovered — text that is not JSON, not
an array, carries no `messageTypeId` of 2, or whose `uniqueId` is not a string —
shall be logged and skipped with no reply.

**OC-R-100** — A malformed CallResult or CallError frame shall never be answered,
whether or not its id is recoverable: a CallError about a CallError is not a
valid exchange, and a CallResult has no pending call on the peer to fail.

---

## Security

**OC-R-029** — Three OCPP security profiles shall be supported: Profile 1 (HTTP
Basic Auth over plain `ws://`), Profile 2 (TLS with a server certificate only),
and Profile 3 (mutual TLS).

**OC-R-030** — A CS with Basic Auth configured shall send an
`Authorization: Basic <base64(user:pass)>` header on the WebSocket upgrade
request.

**OC-R-031** — A CSMS with Basic Auth configured shall reject any upgrade request
whose `Authorization` header is absent or does not match the configured
credentials, answering HTTP 401 and never disclosing the expected credential.

**OC-R-032** — A CSMS shall reject an upgrade request that does not advertise the
version's subprotocol token, answering HTTP 400. On acceptance it shall echo the
token in the `Sec-WebSocket-Protocol` response header.

**OC-R-033** — A Basic Auth password shall never appear in a log line, including
via debug formatting.

**OC-R-034** — A CS TLS configuration shall trust the webpki root store, plus the
certificates in a configured `ca_file` when one is set.

**OC-R-035** — A CS shall present a client certificate for mutual TLS when, and
only when, both a client certificate file and its matching key file are
configured.

**OC-R-036** — A CS with `insecure_skip_verify` set shall accept any server
certificate without authenticating it, and shall ignore `ca_file` entirely. The
handshake's signature verification shall still be performed; only the
certificate-chain/identity check is skipped.

**OC-R-037** — A CSMS TLS server certificate shall come either from PEM files on
disk or from an ephemeral self-signed certificate generated in memory at each
server start and never written to disk.

**OC-R-038** — A generated self-signed CSMS certificate shall carry the listener's
configured host as a subject-alternative name, plus `localhost` when the host
differs from it.

**OC-R-039** — A CSMS with `require_client_cert` set shall reject any client that
does not present a certificate signed by the configured `client_ca_file`.
`require_client_cert` without a `client_ca_file` shall fail the server's start.

**OC-R-040** — `require_client_cert` combined with a self-signed CSMS certificate
shall fail the server's start: there is no CA to distribute for mutual TLS in
that mode.

**OC-R-041** — Failing to open, parse, or find a certificate or private key in a
configured PEM file shall fail the start of the CS connection or the CSMS
listener with a TLS error, before any socket work.

**OC-R-042** — The endpoint scheme shall be authoritative for a server's
transport: a `wss://` endpoint shall bind a TLS-terminated listener, and a
`ws://` endpoint shall bind plain TCP even when server certificate files or
`self_signed` are configured — in that case the TLS material shall be inert.
An endpoint URL shall never advertise a transport its listener does not speak.

**OC-R-097** — The endpoint scheme shall be authoritative for a client's transport
in the same way: a `ws://` CS endpoint shall connect in plaintext and ignore any
configured TLS material. The two roles shall not differ in how they treat the
scheme.

**OC-R-095** — A `wss://` endpoint in the **server** role for which no TLS
material at all is configured shall bind with an ephemeral self-signed
certificate, and the fallback shall be reported in the module log. A `wss://`
server shall never silently bind plain TCP.

**OC-R-096** — Which TLS material a `wss://` server uses shall be decided by its
security configuration: the server certificate + key files when both are set,
otherwise `self_signed`, otherwise the ephemeral fallback of OC-R-095.

---

## Role — Charging Station (CS, client)

**OC-R-043** — A CS shall dial a full WebSocket URL (scheme, host, port, path),
advertising exactly its version's subprotocol token.

**OC-R-044** — The charge-point identity shall be conveyed as the last non-empty
path segment of the URL.

**OC-R-045** — A CS shall accept commands on a command channel while connected:
send a Call and await its typed reply, send a Call without awaiting, and
terminate.

**OC-R-046** — A CS shall answer CSMS-originated Calls through a handler, and
shall expose connect and disconnect lifecycle hooks.

**OC-R-047** — Terminating a CS, or closing its command channel, shall tear the
connection down and end the client task successfully.

**OC-R-048** — A CS shall never reconnect on its own: a dropped or failed
connection stays down until an operator restarts it (see
[`edge-cases.md`](./edge-cases.md)).

---

## Role — CSMS (server)

**OC-R-049** — A CSMS shall bind a TCP listener on a configured host and port and
accept CS connections in a loop, serving each accepted connection concurrently.
A port of `0` shall bind an OS-assigned port, and the bound address shall be
retrievable.

**OC-R-050** — When TLS is configured, every accepted socket shall be
TLS-terminated before the WebSocket handshake is attempted.

**OC-R-051** — Each accepted connection shall be assigned an opaque, monotonically
increasing connection id starting at 1. The charge-point identity parsed from the
URL path shall be kept as metadata against that id, and shall **not** be used as
the connection key — so reconnects and duplicate identities never collide.

**OC-R-052** — A CSMS shall accept commands: send a Call to one connection with
or without awaiting its reply, broadcast a fire-and-forget Call to every live
connection, disconnect one connection, and terminate.

**OC-R-053** — Terminating a CSMS shall terminate every live connection and end
the accept loop.

**OC-R-054** — A connection shall be deregistered from the registry when its
connection loop ends, for any reason.

**OC-R-055** — A command addressing an unknown connection id shall fail that
command alone: an awaited Call shall receive an `InternalError` rejection, a
fire-and-forget Call shall be logged and dropped. The server shall keep running.

**OC-R-056** — A CSMS shall answer CS-originated Calls through a handler that is
told which connection the Call arrived on, so one handler can serve many
concurrently connected charging stations.

---

## Simulated Charging Station behavior

**OC-R-057** — A CS module shall maintain charge-point-wide state (model, vendor,
firmware version, serial number, a configuration/variable key store, the
CSMS-supplied heartbeat cadence, a charge-point-level reservation) and a list of
connector states, all multiplexed over the single WebSocket.

**OC-R-058** — Each connector shall carry its own metering, status, transaction,
per-purpose charging limits, RFID tag, and reservation.

**OC-R-059** — A defined subset of CS-originated actions shall be *state-driven*:
their request is built entirely from the observed state and is sent without
opening a dialog. All other CS-originated actions shall be sent through a dialog.

**OC-R-060** — While connected, the CS shall send Heartbeat automatically at the
cadence the CSMS returned in its BootNotification response, falling back to 30 s
when that value is absent or zero, and never faster than 1 s.

**OC-R-061** — While connected, the CS shall send MeterValues automatically about
every 5 s for each connector with a live transaction, and shall send none when no
transaction is live.

**OC-R-062** — Losing the connection shall halt all automatic transmission and
reset the heartbeat cadence counter.

**OC-R-063** — An inbound Call carrying a top-level connector/EVSE id that this
charging station does not have shall be rejected with CallError
`PropertyConstraintViolation`. Id `0`, and an absent id, shall always be valid
and shall mean the charge point itself.

**OC-R-064** — An inbound Call the CS simulator does not model shall be
default-accepted with the action's `Default`-derived response, not rejected.

**OC-R-065** — The CS shall answer configuration reads from its key store: a
request naming keys shall return the known ones and list the unknown ones; a
request naming no keys shall return every key.

**OC-R-066** — A configuration write shall update an existing writable key, be
rejected for a read-only key, and create the key when it does not exist.

**OC-R-067** — An inbound charging-profile installation shall be rejected when its
stack level exceeds the configured maximum stack level, and otherwise shall apply
its limit to the targeted connector under the field matching the profile's
purpose. Absent that configuration key, no ceiling shall be enforced.

**OC-R-068** — Clearing charging profiles shall erase only the per-purpose limit
matching the request's purpose criterion, or every per-purpose limit when no
purpose is given. An unrecognized purpose shall clear nothing.

**OC-R-069** — A reservation shall be recorded at the level the request targets
(charge point or connector) and shall be cleared by a cancellation carrying the
same reservation id, at whichever level holds it.

**OC-R-070** — A remotely started transaction shall mint a local transaction id,
put the targeted connector into a charging state, and — absent an explicit target
— use the first connector. A remote stop shall clear the transaction, clear the
transaction-scoped charging limit, and return the connector to available.

**OC-R-071** — A reset shall return every connector to available, clear its
transaction, and zero its session energy.

**OC-R-072** — Ending a transaction shall clear only the transaction-scoped
charging limit; the default and maximum limits shall persist.

---

## Simulated CSMS behavior

**OC-R-073** — A CSMS module shall answer every CS-originated Call. Four actions
shall be answered with a crafted response rather than the default: boot
notification (accepted, with the current time and a heartbeat interval),
heartbeat (the current time), authorization (an accept/reject status), and
transaction start (a freshly minted, unique transaction id plus an accept/reject
status). Every other CS-originated Call shall be answered with the action's
`Default`-derived response.

**OC-R-074** — A CSMS shall maintain RFID accept-lists at two levels: one
charge-point-wide list and one per connector/EVSE. A connector's effective set
shall be its own list unioned with the charge-point-wide list.

**OC-R-075** — An empty effective accept-set shall accept every tag. A non-empty
effective set shall accept only the tags it lists.

**OC-R-076** — An authorization request, which names no connector, shall be
checked against the charge-point-wide list unioned with **every** connector list.
A transaction start, which names a connector, shall be checked against that
connector's effective set only — one connector's tags shall not authorize
another's.

**OC-R-077** — A CSMS shall observe every connected station's connectors from the
inbound traffic and shall track them per connection; connectors shall not be
pre-configured for the server role.

**OC-R-078** — Every inbound Call and the reply to it, and every outbound Call and
the reply to it, shall be recorded for display and logging, tagged with the
charge-point/connector scope it belongs to.

---

## Module lifecycle and configuration

**OC-R-079** — An OCPP module instance shall be either a charging station
(client) or a management system (server), never both, and shall speak exactly one
OCPP version.

**OC-R-080** — The OCPP version shall be a property of the **device config**, not
the session entry, because a device's simulation scripts call version-specific
actions and are therefore version-locked.

**OC-R-081** — The session entry shall carry only the instance name, the device
config path, and the endpoint (scheme, ip, port, path). Version, role, timeout,
security, scripts, connectors, and configuration keys shall live in the device
config.

**OC-R-082** — The connection or listener configuration shall be rebuilt from the
current module spec on every start, so an edited endpoint or security section
always takes effect on the next start without a stale copy.

**OC-R-083** — A client module shall **not** connect automatically; it shall
connect only on an explicit start. A server module shall bind its listener
automatically on creation, and a failed bind shall be logged and shall not be
retried automatically.

**OC-R-084** — Restarting a module shall stop the current instance and start a new
one from the current spec. Restarting a server shall additionally discard every
observed charging-station entry.

**OC-R-085** — Changing a module's role, or its OCPP version, shall replace the
view with one built for the new role/version. Changing anything else shall
reconfigure the running instance in place, reconnecting only if it was connected.

**OC-R-086** — Switching a client's OCPP version shall keep its Lua scripts and
warn that they may call actions the new version lacks.

**OC-R-087** — Each module shall keep a bounded in-memory message log of the most
recent 200 messages, evicting oldest-first; the complete history shall be
preserved only in the configured log file.

**OC-R-088** — When file logging is enabled, each message shall be written to the
module's log file at most once, tracked by its sequence number, so that eviction
from the in-memory buffer does not cause a message to be logged twice or skipped
(see [`edge-cases.md`](./edge-cases.md) §6.11 for the burst bound).

**OC-R-101** — Encoding an OCPP action or response to JSON for the message log
shall never discard an encode failure silently: the failure shall be logged to the
module's error channel before the payload degrades to JSON `null`.

---

## Send dialogs

**OC-R-089** — Every action reachable through a send dialog shall be classified as
exactly one of: *typed* (a flat property table with a per-property kind, prefill
source, and optionality) or *raw JSON*. Neither silent omission nor dual
classification shall be possible.

**OC-R-090** — An action shall be classified raw-JSON when, and only when, its
request's required fields include a nested object, or a repeated list with no
optional escape hatch — payload shapes the flat property table cannot express.

**OC-R-091** — Every raw-JSON action shall ship a template payload that decodes
and validates against its own version's request type.

**OC-R-092** — A typed dialog shall always additionally offer a raw-JSON mode,
prefilled from the current property rows.

**OC-R-093** — An action whose required fields are a nested shape that a small
number of flat fields can nonetheless drive shall be permitted a typed dialog with
a custom assembler that folds those flat fields into the full nested request.

**OC-R-094** — A payload assembled by a dialog shall be validated by decoding it
against the version's request type before it is sent; a payload that fails to
decode shall be reported and shall not be sent.
