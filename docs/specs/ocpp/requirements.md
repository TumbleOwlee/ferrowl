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

**OC-R-001** — Exactly three OCPP versions shall be supported: 1.6, 2.0.1, and 2.1. Every version shall be reachable in both roles.

**OC-R-002** — Each version shall declare its complete action table: exactly 28 actions for 1.6, 64 for 2.0.1, and 90 for 2.1.

**OC-R-003** — Each version's action table shall partition into a CS-originated set and a CSMS-originated set: the two sets shall be disjoint and together shall cover every action in the table.

**OC-R-004** — Each version shall declare exactly one WebSocket subprotocol token: `ocpp1.6`, `ocpp2.0.1`, `ocpp2.1`. The token shall be fixed by the version and shall never be configurable.

**OC-R-005** — Every CSMS-originated action shall carry a connector scope of `None` (no connector/EVSE field), `Optional` (the field exists but may be omitted), or `Required` (the field is mandatory). The scope shall be derived from the presence and optionality of the request's top-level connector/EVSE target.

**OC-R-006** — Each action shall expose a `Default`-derived request template and a `Default`-derived response template, retrievable by wire action name. An unknown name shall yield no template rather than an error.

**OC-R-007** — 2.1 shall be a strict superset of 2.0.1: every 2.0.1 action name shall also exist in 2.1, and the 26 additional 2.1 actions shall be additive.

**OC-R-008** — A version's own request-validation rules shall be applied on every inbound Call before it reaches a handler, and shall not be applied to outbound Calls.

---

## OCPP-J framing

**OC-R-009** — The wire format shall be OCPP-J: JSON arrays carried in WebSocket **text** frames. Exactly three envelope shapes shall exist: Call `[2, uniqueId, action, payload]`, CallResult `[3, uniqueId, payload]`, and CallError `[4, uniqueId, errorCode, errorDescription, errorDetails]`.

**OC-R-010** — Decoding shall reject a frame that is not valid JSON, is not a JSON array, carries an unknown message-type id, has the wrong element count for its message type (4 / 3 / 5 respectively), or whose `uniqueId`, `action`, or `errorCode` element is not a JSON string.

**OC-R-011** — A unique id shall be an arbitrary string. Every outbound Call shall generate a fresh UUID v4 unique id; an inbound Call's unique id shall be echoed back verbatim on the reply, whatever its form.

**OC-R-012** — The `errorCode` element shall be one of exactly ten spec-fixed codes (see [`api-contract.md`](./api-contract.md)). An unrecognized code received on the wire shall be accepted and mapped to `GenericError` rather than failing the frame.

**OC-R-013** — Non-text WebSocket frames (binary, ping, pong) shall be ignored without error and shall not be treated as OCPP-J payloads.

---

## Connection engine

**OC-R-014** — A connection shall be full-duplex: both peers may originate Calls concurrently on the same socket, and the engine shall be role-agnostic — the CS and CSMS roles shall share one connection implementation.

**OC-R-015** — Outbound frames shall be serialized through a single writer, so frames are never interleaved on the wire.

**OC-R-016** — Each inbound Call shall be dispatched in its own task, so a slow or re-entrant handler shall never block the reading of further frames.

**OC-R-017** — An outbound Call awaiting a reply shall be registered in a correlation table keyed by its unique id. An inbound CallResult or CallError shall complete the matching entry.

**OC-R-018** — A CallResult carries no action name; the reply shall therefore be decoded using the originating action to select the response type.

**OC-R-019** — A CallResult or CallError whose unique id matches no pending entry shall be discarded silently and shall not disturb the connection.

**OC-R-020** — Every awaited outbound Call shall be bounded by the configured reply timeout. On expiry the correlation entry shall be discarded and the caller shall receive a `GenericError` rejection.

**OC-R-021** — A Call may also be sent fire-and-forget, with no correlation entry and no reply delivered to the caller.

**OC-R-022** — On connection teardown every still-pending outbound Call shall be failed with a `GenericError` rejection, so no caller is left waiting.

**OC-R-023** — A peer's WebSocket close, or a transport error while reading, shall end the connection: the role's command loop shall be signalled, the connection shall be torn down, and the role's disconnect hook shall fire.

**OC-R-024** — A malformed inbound frame shall be logged and shall not tear down the connection. It is otherwise skipped, except where OC-R-098 requires it to be answered with a CallError.

**OC-R-121** — Connection teardown shall not block on an in-flight inbound Call handler task: `shutdown()` shall abort every still-running handler rather than await it. A handler aborted this way never sends its reply.

---

## Errors

**OC-R-025** — A handler shall be able to reject an inbound Call at the protocol level by returning a call error (code, description, details). This shall be sent back as a CallError frame and shall leave the connection intact.

**OC-R-026** — An inbound Call naming an action the negotiated version does not have shall be answered with CallError `NotImplemented`.

**OC-R-027** — An inbound Call whose payload fails to deserialize into the action's request type, or fails the version's validation rules, shall be answered with CallError `FormationViolation`.

**OC-R-028** — A CallError received in reply to an outbound Call shall be surfaced to the caller as a rejection carrying the peer's code, description, and details verbatim — it shall not be turned into a connection failure.

**OC-R-098** — A frame that fails to decode but is identifiable as a Call (`messageTypeId` 2) whose `uniqueId` is a string shall be answered with a CallError carrying that id and the code `FormationViolation`. A peer that sent a recoverable Call shall never be left to wait out its own call timeout.

**OC-R-099** — A frame whose id cannot be recovered — text that is not JSON, not an array, carries no `messageTypeId` of 2, or whose `uniqueId` is not a string — shall be logged and skipped with no reply.

**OC-R-100** — A malformed CallResult or CallError frame shall never be answered, whether or not its id is recoverable: a CallError about a CallError is not a valid exchange, and a CallResult has no pending call on the peer to fail.

---

## Security

**OC-R-029** — Three OCPP security profiles shall be supported: Profile 1 (HTTP Basic Auth over plain `ws://`), Profile 2 (TLS with a server certificate only), and Profile 3 (mutual TLS).

*Coverage note: OC-R-029 is an umbrella statement over the three security profiles; its substance is independently covered by the per-profile requirements and their tests — OC-R-030/OC-R-031 (Profile 1, Basic Auth, `ferrowl-ocpp/tests/ws_loopback_security.rs`), OC-R-096 (Profile 2, TLS with only a server certificate, `ferrowl/src/module/ocpp/config/{session,device}.rs`), and OC-R-039/OC-R-040 (Profile 3, mutual TLS, `ws_loopback_security.rs` and `ferrowl-ocpp/src/security.rs`). No separate test cites OC-R-029 itself.*

**OC-R-030** — A CS with Basic Auth configured shall send an `Authorization: Basic <base64(user:pass)>` header on the WebSocket upgrade request.

**OC-R-031** — A CSMS with Basic Auth configured shall reject any upgrade request whose `Authorization` header is absent or does not match the configured credentials, answering HTTP 401 and never disclosing the expected credential.

**OC-R-032** — A CSMS shall reject an upgrade request that does not advertise the version's subprotocol token, answering HTTP 400. On acceptance it shall echo the token in the `Sec-WebSocket-Protocol` response header.

**OC-R-033** — A Basic Auth password shall never appear in a log line, including via debug formatting.

**OC-R-034** — A CS TLS configuration's trust anchors shall be exactly those its `verification` variant names: `CertVerification::RootStore` trusts the webpki root store plus every `extra_ca_files` entry, `CertVerification::CaFiles` trusts exactly the named `ca_files` and not the webpki root store, and `CertVerification::Skip` names no trust anchor at all (OC-R-036).

**OC-R-035** — A CS shall present a client certificate when, and only when, its policy is `ClientTlsPolicy::Mutual`; `ClientTlsPolicy::Tls` and `ClientTlsPolicy::None` shall present none. What it presents follows that policy's `identity` variant: `CertSource::Files` presents the PEM pair at `cert_file`/`key_file`, and `CertSource::SelfSigned` presents the cached ephemeral pair of OC-R-115. `CertSource::Ephemeral` shall be rejected at construction as a client identity, mirroring MB-R-110.

**OC-R-036** — A CS whose `verification` is `CertVerification::Skip` shall accept any server certificate without authenticating it; the variant names no trust anchor, so there is nothing to ignore. The handshake's signature verification shall still be performed; only the certificate-chain/identity check is skipped. Under `CertVerification::RootStore` the server certificate shall be verified against the native root store plus every `extra_ca_files` entry; under `CertVerification::CaFiles`, against exactly the named `ca_files` and not the native root store, `ca_files` being non-empty.

**OC-R-037** — A CSMS TLS server certificate shall come either from the PEM files of `CertSource::Files` or from an ephemeral self-signed certificate (`CertSource::SelfSigned` or `CertSource::Ephemeral`). The self-signed pair shall be generated once and cached for the life of the module instance, reused across every bind attempt, reconnect-driven rebind, and `:restart`/`:reload`, and across a configuration edit that leaves the identity on a self-signed variant — regenerated only when the identity changes to `SelfSigned`/`Ephemeral` from `Files`, or the endpoint's TLS mode transitions from `None` to `Tls`/`Mutual` while the identity is already self-signed. A module instance being torn down and freshly constructed discards the cache. Never written to disk.

**OC-R-038** — A generated self-signed CSMS certificate shall carry the listener's configured host as a subject-alternative name, plus `localhost` when the host differs from it.

**OC-R-039** — With a CSMS's `ServerTlsPolicy::Mutual`, verification of the presented client certificate is governed by `verification`. Under `CertVerification::CaFiles`, a client presenting no certificate, or one not signed by any of the configured `ca_files`, shall be rejected — a certificate signed by any one configured CA is sufficient, not all; `ca_files` shall be non-empty, construction failing otherwise. `CertVerification::RootStore` shall be rejected at construction on this path as client-only. Under `CertVerification::Skip`, a client presenting no certificate shall still be rejected, but a presented certificate shall be accepted without CA validation — signature check still performed, only chain/identity check skipped.

**OC-R-040** — `ServerTlsPolicy::Mutual` combined with a self-signed CSMS certificate (`identity: CertSource::SelfSigned`) shall be permitted regardless of which `verification` mode is configured — `CertVerification::CaFiles` (with at least one CA file, construction-checked) or `CertVerification::Skip` — since the server's own self-signed identity and its client-verification mode are independent. `CaFiles` with zero CA files remains rejected at construction (OC-R-039), regardless of certificate mode.

**OC-R-041** — Failing to open, parse, or find a certificate or private key in a configured PEM file shall fail the start of the CS connection or the CSMS listener with a TLS error, before any socket work.

**OC-R-042** — The endpoint scheme shall be authoritative for a server's transport: a `wss://` endpoint shall bind a TLS-terminated listener, and a `ws://` endpoint shall bind plain TCP even when a `ServerTlsPolicy` other than `None` is configured — in that case the whole TLS policy, identity and verification alike, shall be inert. An endpoint URL shall never advertise a transport its listener does not speak.

**OC-R-097** — The endpoint scheme shall be authoritative for a client's transport in the same way: a `ws://` CS endpoint shall connect in plaintext and ignore any configured TLS material. The two roles shall not differ in how they treat the scheme.

**OC-R-095** — A `wss://` endpoint in the **server** role whose `identity` is `CertSource::Ephemeral` — the variant standing for "no TLS material configured" — shall bind with an ephemeral self-signed certificate, and the fallback shall be reported in the module log. `CertSource::SelfSigned` shall bind the same way without logging a fallback, the selection being explicit. A `wss://` server shall never silently bind plain TCP.

**OC-R-096** — Which TLS material a `wss://` server uses shall be decided by its `identity` variant: `CertSource::SelfSigned` presents an ephemeral self-signed certificate, `CertSource::Files` presents the named certificate + key files, and `CertSource::Ephemeral` presents the logged fallback of OC-R-095. Cadence for the self-signed pair follows OC-R-037.

**OC-R-110** — The OCPP setup dialog shall offer a Self-Signed toggle for the server role, shown whenever the security level is `Tls` or `Mutual` (mirroring the Modbus TCP dialog's Self-Signed toggle). When On, the server certificate/key inputs (`cert_file`/`key_file`) shall be hidden and the resolved identity shall be `CertSource::SelfSigned` regardless of any text already present in them; the dialog's validation shall not require those files while Self-Signed is On. Self-Signed shall have no effect on the client-CA list or on the mTLS selection, which continues to control the `Mutual` variant independently. The input widgets' stored text shall be left unmodified, so toggling Self-Signed back Off restores the previously entered cert/key paths and re-requires them for validation.

**OC-R-111** — The OCPP setup dialog's client-side Root Store toggle and server-CA list (OC-R-125) shall both be hidden whenever Skip-Verify is On (mirroring the Modbus TCP dialog), the resolved verification then being `CertVerification::Skip`, which carries neither. The hidden widgets' stored state — toggle position and list entries alike — shall be left unmodified, so toggling Skip-Verify back Off restores exactly what was previously entered. The Skip-Verify toggle itself (client role) shall be shown only when the security level is `Tls` or `Mutual`, hidden under `None`.

**OC-R-112** — A half-configured certificate pair is no longer representable in a configuration file: `CertSource::Files` carries both `cert_file` and `key_file` as required fields, so a document naming one without the other fails to deserialize. The rule survives only in the setup dialog, whose inputs are editable independently: the dialog shows an error border on a blank cert/key input while shown, and refuses to close on submit while either of a role's pair is blank, for both the CSMS identity and the CS mTLS identity, mirroring MB-R-107.

**OC-R-113** — The OCPP setup dialog shall present CA file paths through one shared list widget, used by the server (CSMS) role for the client-CA list (shown whenever mTLS is selected) and by the client (CS) role for the server-CA list (OC-R-125), mirroring the Modbus TCP dialog's equivalent list (MB-R-136) and allowing zero or more paths to be added, edited, or removed individually. Confirming an add-entry attempt shall be rejected — leaving the entry sub-dialog open with an inline error instead of appending anything — unless the typed path is non-empty, exists on disk, is not a directory, and has one of the accepted certificate-file extensions (`pem`/`crt`/`key`, case-insensitive), mirroring MB-R-136's same gate on its shared add-CA sub-dialog. For the server role, selecting mTLS with a non-empty list and Skip Verify Off resolves to `ServerTlsPolicy::Mutual` with `CertVerification::CaFiles` holding exactly those files; an empty list at that point is a validation error. The dialog shall additionally offer a Skip Verify toggle for the server (CSMS) role, shown whenever mTLS is selected: when On, the client-CA list shall be hidden and the resolved verification shall be `CertVerification::Skip` regardless of entries already present — mirroring MB-R-136's hidden-field-exclusion pattern (list contents preserved, restored when the toggle goes back Off).

**OC-R-115** — Under `ClientTlsPolicy::Mutual` with `identity: CertSource::SelfSigned`, a CS shall present an ephemeral self-signed certificate/key pair as its mTLS identity, generated and cached per the rule in OC-R-037 (generated once per module instance, reused across reconnects/restarts/config edits, regenerated only on a transition into self-signed), never written to disk.

**OC-R-116** — The OCPP setup dialog shall offer a Self Signed toggle for the CS (client) role, shown whenever mTLS is selected. When On, the Client Cert/Key inputs shall be hidden and excluded from the resolved config regardless of text already present, resolving to `ClientTlsPolicy::Mutual`'s `identity: CertSource::SelfSigned`; the dialog's validation shall not require those files while Self Signed is On. The input widgets' stored text shall be left unmodified, so toggling Self Signed back Off restores the previously entered paths and re-requires them for validation.

**OC-R-125** — The OCPP setup dialog's client (CS) role shall offer, whenever Skip-Verify is Off and the security level is `Tls` or `Mutual`, a Root Store toggle (default On) together with the shared CA list widget (OC-R-113) holding zero or more server-CA paths — replacing the single `ca_file` input. Root Store On resolves to `CertVerification::RootStore` with the list as `extra_ca_files`, empty or not; Root Store Off resolves to `CertVerification::CaFiles` with the list as `ca_files`, and an empty list at that point shall be a validation error refusing to close the dialog, mirroring OC-R-113's empty-list rule on the server side. Skip-Verify On shall hide both the toggle and the list per OC-R-111.

**OC-R-126** — The OCPP device security config shall carry its TLS material as a `tls` sub-block holding one `ServerTlsPolicy` under `server` and one `ClientTlsPolicy` under `client`, serialized as `[security.tls.server]` and `[security.tls.client]`, replacing the flat role-mixed certificate fields that sat directly beside `username`/`password`. `username` and `password` shall remain flat members of `security`, shared by both roles as before. Each policy shall independently default to its own `None` variant, so an absent `[security.tls]` block is exactly both policies `None`. A CS or CSMS shall decide whether TLS is configured by matching its own role's policy variant, never by comparing the security block as a whole against an all-unset baseline; the policy for the other role shall be inert, never validated against this role's rules. The endpoint scheme remains authoritative over the policy (OC-R-042).

**OC-R-117** — A CS device config may declare `extra_headers`, an ordered list of `HeaderDef { name, value }` pairs. Every configured header shall be sent on the WebSocket upgrade request in addition to any header the client sets itself (e.g. the Basic Auth `Authorization` header of OC-R-030, the subprotocol token of OC-R-004). Construction shall reject a `HeaderDef` whose `name` case-insensitively matches a header name the client itself controls (`Authorization`, `Host`, `Upgrade`, `Connection`, `Sec-WebSocket-Key`, `Sec-WebSocket-Version`, `Sec-WebSocket-Protocol`, `Sec-WebSocket-Extensions`), naming the offending header in the error.

**OC-R-118** — A `HeaderDef.name` shall match the HTTP token grammar (visible ASCII, no separators or whitespace); a `HeaderDef.value` shall contain only printable ASCII bytes (0x20–0x7E), which excludes CR/LF and other control characters. Construction shall reject any `HeaderDef` violating either rule, naming the offending header and field in the error.

**OC-R-119** — `extra_headers` is a client-only device config field. It shall not be exposed through the `--ocpp` key=value CLI form — only through the device config file — consistent with the other list-shaped client-only fields (`connectors`, `config`).

**OC-R-120** — A CS's connection status lines (e.g. "Client disconnected", emitted whenever its connection task ends) shall be written to the module log, not the message log — the message log records only request/response message pairs (§9).

---

## Role — Charging Station (CS, client)

**OC-R-043** — A CS shall dial a full WebSocket URL (scheme, host, port, path), advertising exactly its version's subprotocol token.

**OC-R-044** — The charge-point identity shall be conveyed as the last non-empty path segment of the URL.

**OC-R-045** — A CS shall accept commands on a command channel while connected: send a Call and await its typed reply, send a Call without awaiting, and terminate.

**OC-R-046** — A CS shall answer CSMS-originated Calls through a handler, and shall expose connect and disconnect lifecycle hooks.

**OC-R-047** — Terminating a CS, or closing its command channel, shall tear the connection down and end the client task successfully.

**OC-R-048** — With `reconnect` enabled (the default), a CS shall never end its task on a failed dial or a dropped connection; it shall wait a backoff and retry the connection, using the same backoff policy as the Modbus client (MB-R-051). With `reconnect` disabled, a failed dial or a dropped connection shall end the CS task with that error, after emitting a disconnected status.

**OC-R-105** — A CS's reconnect backoff shall reset to 1 s after any connection during which the WebSocket handshake completed, regardless of whether any OCPP message was subsequently exchanged.

**OC-R-106** — Terminating a CS, or closing its command channel, while it is backing off shall abort the wait immediately and end the CS task with success (extends OC-R-047 to the backing-off state).

**OC-R-107** — The `reconnect` field, the endpoint, and the security (TLS/auth) configuration shall be re-read from the shared device config on every dial attempt, so an edit to them takes effect on the next reconnect without a restart (mirrors MB-R-056).

**OC-R-114** — With `reconnect` enabled, a CS shall log the failure reason for each failed dial or dropped connection, and shall log a line stating the wait duration before each backoff wait, mirroring the Modbus client's reconnect logging (MB-R-051).

**OC-R-123** — A CS module's displayed connection status shall follow the same three-state rule as a Modbus client module's (MB-R-137): `CONNECTED` while the WebSocket connection is currently open; `RECONNECTING` while the CS task is running but not currently connected (a dial attempt in progress or a reconnect backoff wait, OC-R-048); `DISCONNECTED` while the CS task is not running.

---

## Role — CSMS (server)

**OC-R-049** — A CSMS shall bind a TCP listener on a configured host and port and accept CS connections in a loop, serving each accepted connection concurrently. A port of `0` shall bind an OS-assigned port, and the bound address shall be retrievable.

**OC-R-050** — When TLS is configured, every accepted socket shall be TLS-terminated before the WebSocket handshake is attempted.

**OC-R-051** — Each accepted connection shall be assigned an opaque, monotonically increasing connection id starting at 1. The charge-point identity parsed from the URL path shall be kept as metadata against that id, and shall **not** be used as the connection key — so reconnects and duplicate identities never collide.

**OC-R-052** — A CSMS shall accept commands: send a Call to one connection with or without awaiting its reply, broadcast a fire-and-forget Call to every live connection, disconnect one connection, and terminate.

**OC-R-053** — Terminating a CSMS shall terminate every live connection and end the accept loop.

**OC-R-108** — A CSMS's listener-bind backoff shall reset to 1 s once the listener has bound and accepted at least one connection.

**OC-R-109** — Terminating a CSMS while it is backing off from a failed bind shall abort the wait immediately and end the module task successfully (extends OC-R-053 to the backing-off state).

**OC-R-054** — A connection shall be deregistered from the registry when its connection loop ends, for any reason.

**OC-R-055** — A command addressing an unknown connection id shall fail that command alone: an awaited Call shall receive an `InternalError` rejection, a fire-and-forget Call shall be logged and dropped. The server shall keep running.

**OC-R-056** — A CSMS shall answer CS-originated Calls through a handler that is told which connection the Call arrived on, so one handler can serve many concurrently connected charging stations.

---

## Simulated Charging Station behavior

**OC-R-057** — A CS module shall maintain charge-point-wide state (model, vendor, firmware version, serial number, a configuration/variable key store, the CSMS-supplied heartbeat cadence, a charge-point-level reservation) and a list of connector states, all multiplexed over the single WebSocket.

**OC-R-058** — Each connector shall carry its own metering, status, transaction, per-purpose charging limits, RFID tag, and reservation.

**OC-R-059** — A defined subset of CS-originated actions shall be *state-driven*: their request is built entirely from the observed state and is sent without opening a dialog. All other CS-originated actions shall be sent through a dialog.

**OC-R-060** — While connected, the CS shall send Heartbeat automatically at the cadence the CSMS returned in its BootNotification response, falling back to 30 s when that value is absent or zero, and never faster than 1 s.

**OC-R-061** — While connected, the CS shall send MeterValues automatically about every 5 s for each connector with a live transaction, and shall send none when no transaction is live.

**OC-R-062** — Losing the connection shall halt all automatic transmission and reset the heartbeat cadence counter.

**OC-R-063** — An inbound Call carrying a top-level connector/EVSE id that this charging station does not have shall be rejected with CallError `PropertyConstraintViolation`. Id `0`, and an absent id, shall always be valid and shall mean the charge point itself.

**OC-R-064** — An inbound Call the CS simulator does not model shall be default-accepted with the action's `Default`-derived response, not rejected.

**OC-R-065** — The CS shall answer configuration reads from its key store: a request naming keys shall return the known ones and list the unknown ones; a request naming no keys shall return every key.

**OC-R-066** — A configuration write shall update an existing writable key, be rejected for a read-only key, and create the key when it does not exist.

**OC-R-067** — An inbound charging-profile installation shall be rejected when its stack level exceeds the configured maximum stack level, and otherwise shall apply its limit to the targeted connector under the field matching the profile's purpose. Absent that configuration key, no ceiling shall be enforced.

**OC-R-068** — Clearing charging profiles shall erase only the per-purpose limit matching the request's purpose criterion, or every per-purpose limit when no purpose is given. An unrecognized purpose shall clear nothing.

**OC-R-069** — A reservation shall be recorded at the level the request targets (charge point or connector) and shall be cleared by a cancellation carrying the same reservation id, at whichever level holds it.

**OC-R-070** — A remotely started transaction shall mint a local transaction id, put the targeted connector into a charging state, and — absent an explicit target — use the first connector, transmitting the transaction-start message (`StartTransaction` for 1.6, `TransactionEvent` with `eventType=Started` for 2.0.1/2.1) through the same send path the RFID/operator-triggered flow uses, so the CSMS learns the transaction id via the normal wire message in every case — matching 1.6's existing pattern of letting the response assign the id where applicable. A remote stop shall clear the transaction, clear the transaction-scoped charging limit, and return the connector to available.

**OC-R-071** — A reset shall return every connector to available, clear its transaction, and zero its session energy.

**OC-R-122** — Whenever a transaction-start message (`StartTransaction` or `TransactionEvent` with `eventType=Started`) is transmitted — whether triggered by RFID/operator action or by an accepted remote-start (OC-R-070) — the same send shall also transmit a `StatusNotification` for that connector carrying its updated status: `ChargePointStatus::Charging` for 1.6, `ConnectorStatusEnumType::Occupied` for 2.0.1/2.1.

**OC-R-072** — Ending a transaction shall clear only the transaction-scoped charging limit; the default and maximum limits shall persist.

---

## Simulated CSMS behavior

**OC-R-073** — A CSMS module shall answer every CS-originated Call. Four actions shall be answered with a crafted response rather than the default: boot notification (accepted, with the current time and a heartbeat interval), heartbeat (the current time), authorization (an accept/reject status), and transaction start (a freshly minted, unique transaction id plus an accept/reject status). Every other CS-originated Call shall be answered with the action's `Default`-derived response.

**OC-R-074** — A CSMS shall maintain RFID accept-lists at two levels: one charge-point-wide list and one per connector/EVSE. A connector's effective set shall be its own list unioned with the charge-point-wide list.

**OC-R-075** — An empty effective accept-set shall accept every tag. A non-empty effective set shall accept only the tags it lists.

**OC-R-076** — An authorization request, which names no connector, shall be checked against the charge-point-wide list unioned with **every** connector list. A transaction start, which names a connector, shall be checked against that connector's effective set only — one connector's tags shall not authorize another's.

**OC-R-077** — A CSMS shall observe every connected station's connectors from the inbound traffic and shall track them per connection; connectors shall not be pre-configured for the server role.

**OC-R-078** — Every inbound Call and the reply to it, and every outbound Call and the reply to it, shall be recorded for display and logging, tagged with the charge-point/connector scope it belongs to.

---

## Module lifecycle and configuration

**OC-R-079** — An OCPP module instance shall be either a charging station (client) or a management system (server), never both, and shall speak exactly one OCPP version.

**OC-R-080** — The OCPP version shall be a property of the **device config**, not the session entry, because a device's simulation scripts call version-specific actions and are therefore version-locked.

**OC-R-081** — The session entry shall carry only the instance name, the device config path, and the endpoint (scheme, ip, port, path). Version, role, timeout, security, scripts, connectors, configuration keys, and (client role) CS boot identity shall live in the device config.

**OC-R-103** — A charging-station (client) device config shall persist the CS boot identity (model, vendor, firmware version, serial number) the same way it persists connectors and configuration keys: `:wd`/`:write-device` shall write the station's current values, and loading the device config shall seed them into the CS state, overriding the built-in defaults only when the field is present in the file. Absence of a field falls back to its built-in default, so a file predating this field still loads.

**OC-R-104** — In OCPP 1.6, a CS's charge-point-wide state shall additionally carry four optional identity fields: SIM ICCID, SIM IMSI, meter serial number, and meter type. Each shall be persisted by `:wd`/`:write-device` and seeded on device-config load the same way as the existing boot identity fields (OC-R-103), overriding the built-in default (empty/unset) only when the field is present in the file. A field left empty shall be omitted from `BootNotification` entirely; a field holding a value shall be included under its wire name (`iccid`, `imsi`, `meterSerialNumber`, `meterType`). These fields shall not apply to OCPP 2.0.1 or 2.1, which have no equivalent `BootNotification` fields.

**OC-R-082** — The connection or listener configuration shall be rebuilt from the current module spec on every start, so an edited endpoint or security section always takes effect on the next start without a stale copy.

**OC-R-083** — A client module shall **not** connect automatically; it shall connect only on an explicit start. A server module shall bind its listener automatically on creation. With `reconnect` enabled (the default), a failed bind shall not end the module task; it shall retry using the same backoff policy as the Modbus client (MB-R-051). With `reconnect` disabled, a failed bind shall end the module task with that error, surfaced to the caller.

**OC-R-124** — A CSMS server module's displayed connection status shall follow the same three-state rule as a Modbus server module's (MB-R-153): `CONNECTED` while the listener is currently bound; `RECONNECTING` while the server task is running but not currently bound (a bind attempt in progress or a reconnect backoff wait, OC-R-083); `DISCONNECTED` while the server task is not running.

**OC-R-084** — Restarting a module shall stop the current instance and start a new one from the current spec. Restarting a server shall additionally discard every observed charging-station entry.

**OC-R-085** — Changing a module's role, or its OCPP version, shall replace the view with one built for the new role/version. Changing anything else shall reconfigure the running instance in place, reconnecting only if it was connected.

**OC-R-086** — Switching a client's OCPP version shall keep its Lua scripts and warn that they may call actions the new version lacks.

**OC-R-087** — Each module shall keep a bounded in-memory message log of the most recent 200 messages, evicting oldest-first; the complete history shall be preserved only in the configured log file.

**OC-R-088** — When file logging is enabled, each message shall be written to the module's log file at most once, tracked by its sequence number, so that eviction from the in-memory buffer does not cause a message to be logged twice or skipped (see [`edge-cases.md`](./edge-cases.md) §6.11 for the burst bound).

**OC-R-101** — Encoding an OCPP action or response to JSON for the message log shall never discard an encode failure silently: the failure shall be logged to the module's error channel before the payload degrades to JSON `null`.

**OC-R-102** — When an OCPP module view stops or (re)starts its backend as part of a settings change, version switch, or a `stop`/`restart` command, a failure of that stop or start shall be reported in the module message log at Error level rather than silently discarded.

---

## Send dialogs

**OC-R-089** — Every action reachable through a send dialog shall be classified as exactly one of: *typed* (a flat property table with a per-property kind, prefill source, and optionality) or *raw JSON*. Neither silent omission nor dual classification shall be possible.

**OC-R-090** — An action shall be classified raw-JSON when, and only when, its request's required fields include a nested object, or a repeated list with no optional escape hatch — payload shapes the flat property table cannot express.

**OC-R-091** — Every raw-JSON action shall ship a template payload that decodes and validates against its own version's request type.

**OC-R-092** — A typed dialog shall always additionally offer a raw-JSON mode, prefilled from the current property rows.

**OC-R-093** — An action whose required fields are a nested shape that a small number of flat fields can nonetheless drive shall be permitted a typed dialog with a custom assembler that folds those flat fields into the full nested request.

**OC-R-094** — A payload assembled by a dialog shall be validated by decoding it against the version's request type before it is sent; a payload that fails to decode shall be reported and shall not be sent.
