# Bridge Mode — Requirements

`ferrowl bridge`: a headless subcommand relaying Modbus requests between two independently configured interfaces — an **upstream** (server role) and a **downstream** (client role). No persistent register store, no TUI, no Lua/`C_*` module — a pure protocol relay.

Per [`../README.md`](../README.md)'s ownership rules, this area does **not** own: the Modbus wire protocol, register codec, or client/server behavior bridge reuses unchanged ([`../modbus/`](../modbus/)); the `--module`/`--ocpp` mini-language grammar or the headless `run` exit-code/log contract bridge mirrors ([`../cli-headless/`](../cli-headless/)).

---

## Subcommand and scope

**BR-R-001** — The program exposes a `bridge` subcommand (`ferrowl bridge`), a third `SubCommand` variant alongside `migrate` and `run` (`CL-R-010`'s pattern). Invoking it replaces the default action (starting the TUI).

**BR-R-002** — Bridge mode does not start the TUI, load a session file, or touch the Lua/`C_*` module or sim framework. It relays between exactly two configured interfaces and owns no persistent register store.

## Configuration

**BR-R-003** — `bridge` accepts exactly one `--upstream <descriptor>` and exactly one `--downstream <descriptor>`, both required. Missing either is a setup failure (exit 1, `BR-R-013`).

**BR-R-004** — Each descriptor uses the existing `key=val[,key=val...]` mini-language (same grammar as `--module`'s endpoint keys): `transport` in {`tcp`, `rtu`, `rtu_over_tcp`, `ascii_over_tcp`} (default `tcp`) selects the transport; remaining keys match that transport's connection fields (`ip`,`port` for tcp/rtu_over_tcp/ascii_over_tcp; `path`,`baud`,`parity`,`data_bits`,`stop_bits` for rtu), plus optional `timeout_ms`, `reconnect`, optional `unit_ids` (`BR-R-015`), and — tcp/rtu_over_tcp/ascii_over_tcp only — the `tls` key set (`BR-R-011`). `rtu_over_tcp`/`ascii_over_tcp` carry the same `tcp::Config` field set as `tcp` (`MB-R-113`/`MB-R-125`), differing only in framing (RTU/ASCII instead of MBAP); either may be upstream or downstream independently of the other side's transport.

## Roles and relay behavior

**BR-R-005** — Upstream always acts as server: bridge listens for/accepts connections (tcp, rtu_over_tcp, ascii_over_tcp) or serves the opened link (rtu), reusing the existing server accept-loop / single-serial-link behavior unchanged.

**BR-R-006** — Downstream always acts as client: bridge connects (tcp, rtu_over_tcp, ascii_over_tcp) or opens the serial port (rtu) as an ordinary client, including reconnect/backoff (`MB-R-050–056`) when enabled.

**BR-R-007** — Each decoded upstream request is forwarded downstream unmodified (same unit id, function code, address, count) and awaited; the downstream response or exception is relayed back upstream unmodified. No register/bit-count limit beyond each transport's wire format — pure pass-through, no bridge-side cap.

**BR-R-008** — A request to slave id 0 received on an RTU upstream is forwarded downstream and receives no upstream response, matching the RTU-server broadcast-silence rule (`MB-R-103`).

**BR-R-009** — A request forwarded to an RTU downstream addressed to slave id 0 is transmitted fire-and-forget (not awaited), matching the RTU-client broadcast-write rule (`MB-R-102`).

**BR-R-010** — When downstream fails to connect while a forwarded request is outstanding (no established connection/serial link), bridge answers the upstream requester with exception `GatewayPathUnavailable` (0x0A). When downstream is connected but the forwarded request times out or the connection drops before a response, bridge answers `GatewayTargetDeviceFailedToRespond` (0x0B). Both codes exist in the vendored `rust_modbus::ExceptionCode` enum (values 10/11); ordinary ferrowl servers never emit them (`api-contract.md`'s exhaustive list is 0x01–0x04), since only bridge has a second link whose failure must be reported back to a first.

**BR-R-011** — TCP-socket interfaces (`tcp`, `rtu_over_tcp`, `ascii_over_tcp`; upstream and/or downstream) may enable TLS through descriptor keys mirroring the block form path for path, the interface's role fixed by its flag rather than configured: an upstream descriptor carries a `ServerTlsPolicy` (BR-R-005), a downstream a `ClientTlsPolicy` (BR-R-006), neither the two-role container of MB-R-104. Within BR-R-004's unchanged `key=val[,key=val]` grammar each TLS key is one dotted path — `tls.mode` (`none`|`tls`|`mutual`, default `none`), `tls.identity.source` (`ephemeral`|`self-signed`|`files`, default `ephemeral`), `tls.identity.cert_file`, `tls.identity.key_file`, `tls.verification.verify` (`skip`|`root-store`|`ca-files`, default `root-store` with empty `extra_ca_files`), `tls.verification.ca_files`, `tls.verification.extra_ca_files` — and the two CA-list keys take `;` as intra-value delimiter, `,` being the descriptor's key separator. Under `tls.mode=tls` or `tls.mode=mutual`, an absent `tls.identity.source` and an absent `tls.verification.verify` take those defaults wherever the selected variant defines that field, so a descriptor naming only a mode resolves to an ephemeral identity and root-store verification with no extra CA files — a convenience of the descriptor form alone, the block form requiring `identity` and `verification` written out (MB-R-105); a defaulted field is then subject to every rule an explicit one faces. A TLS key whose path the selected variant does not define is a setup failure (exit 1, BR-R-013), as are `tls.verification.verify=root-store` on an upstream descriptor and `tls.identity.source=ephemeral` on a downstream one — the role-only halves of MB-R-105's three remaining checks — and those two rejections apply to a defaulted value exactly as to a written one, so an upstream `tls.mode=mutual` with no `tls.verification.verify` and a downstream `tls.mode=mutual` with no `tls.identity.source` are each a setup failure, while an upstream `tls.mode=tls` (identity defaults to `ephemeral`) and a downstream `tls.mode=tls` (verification defaults to `root-store`) are accepted. Any `tls.*` key on a `transport=rtu` descriptor — the one transport that is not a TCP socket — is likewise a setup failure (exit 1, BR-R-013) naming the offending key, whatever its path or value, including `tls.mode=none`, rather than accepted and ignored: TLS is scoped to TCP-socket interfaces, so a TLS key on a serial descriptor can only be a mistake about which interface is being configured. Resolution, verification, and validation otherwise follow `MB-R-104–109`; the bridge defines no TLS field of its own.

```
--upstream 'transport=tcp,ip=0.0.0.0,port=8502,tls.mode=mutual,tls.identity.source=self-signed,tls.verification.verify=ca-files,tls.verification.ca_files=/etc/ferrowl/a.pem;/etc/ferrowl/b.pem'
```

## Logging and process contract

**BR-R-012** — Bridge drains relayed-request and lifecycle log lines to stdout in the `[<timestamp>] <source> | <message>` format (`CL-R-040`), optionally appends to `--log-file` (`CL-R-041`: create-and-append), and keeps setup/fatal diagnostics on stderr (`CL-R-042`).

**BR-R-013** — Exit codes mirror `run` (`CL-R-030–032`): 1 for setup failure (missing/invalid `--upstream`/`--downstream`, upstream bind/listen/serial-open failure); 0 on `--duration` deadline or Ctrl-C; with `--exit-on-error`, 3 on a drained `[bridge]`-sourced error line.

**BR-R-014** — `bridge` accepts an optional `--duration <secs>`, identical semantics to `run`'s (`CL-R-013` family).

## Multidrop bus safety

**BR-R-015** — The upstream descriptor's optional `unit_ids` key takes a comma-separated list/range (e.g. `unit_ids=1,3,5-8`) of allowed unit ids. When present, only a request to a listed unit id is forwarded (`BR-R-007`); any other is ignored entirely — not forwarded, no upstream response — so another device sharing the upstream link can answer it. When absent, every unit id is forwarded (`MB-R-065`'s no-filter default), appropriate for a dedicated point-to-point link such as TCP-TCP.
