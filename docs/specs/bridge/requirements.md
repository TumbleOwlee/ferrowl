# Bridge Mode — Requirements

Testable requirements for `ferrowl bridge`: a headless subcommand that relays
Modbus requests between two independently-configured interfaces (an
**upstream**, acting as a server, and a **downstream**, acting as a client).
Bridge mode owns no persistent register store, no TUI, and no Lua/`C_*`
module — it is a pure protocol relay.

Per the ownership rules in [`../README.md`](../README.md), this area does
**not** own:

- The Modbus wire protocol, register codec, or the client/server behavior
  bridge reuses unchanged — those are [`../modbus/`](../modbus/).
- The `--module`/`--ocpp` mini-language grammar or the headless `run`
  exit-code/log contract bridge mirrors — those are
  [`../cli-headless/`](../cli-headless/).

---

## Subcommand and scope

**BR-R-001** — The program shall expose a `bridge` subcommand (`ferrowl bridge`), a third `SubCommand` variant alongside `migrate` and `run` (`CL-R-010`'s pattern). Invoking it replaces the default action (starting the TUI) with the bridge action.

**BR-R-002** — Bridge mode shall not start the TUI, load a session file, or touch the Lua/`C_*` module or sim framework. It relays Modbus requests between exactly two configured interfaces and owns no persistent register store of its own.

## Configuration

**BR-R-003** — The `bridge` subcommand shall accept exactly one `--upstream <descriptor>` and exactly one `--downstream <descriptor>` flag, both required. Missing either is a setup failure (exit 1, `BR-R-013`).

**BR-R-004** — Each descriptor uses the existing `key=val[,key=val...]` mini-language (same grammar as `--module`'s endpoint keys): `transport` in {`tcp`, `rtu`, `rtu_over_tcp`, `ascii_over_tcp`} (default `tcp`) selects the interface's transport; remaining keys match that transport's existing connection fields (`ip`,`port` for tcp/rtu_over_tcp/ascii_over_tcp; `path`,`baud`,`parity`,`data_bits`,`stop_bits` for rtu), plus optional `timeout_ms`, `reconnect`, an optional `unit_ids` key (`BR-R-015`), and — tcp/rtu_over_tcp/ascii_over_tcp only — the `tls` key set (`BR-R-011`). `rtu_over_tcp`/`ascii_over_tcp` carry the same `tcp::Config` field set as `tcp` (`MB-R-113`/`MB-R-125`), differing only in on-wire framing (RTU/ASCII instead of MBAP); either may be used as upstream or downstream independently of the other side's transport.

## Roles and relay behavior

**BR-R-005** — The upstream interface always acts in the server role: bridge mode listens for/accepts connections (tcp, rtu_over_tcp, ascii_over_tcp) or serves the opened link (rtu) on upstream, reusing the existing server accept-loop / single-serial-link behavior unchanged.

**BR-R-006** — The downstream interface always acts in the client role: bridge mode connects (tcp, rtu_over_tcp, ascii_over_tcp) or opens the serial port (rtu) on downstream as an ordinary client, including existing reconnect/backoff (`MB-R-050–056`) when enabled.

**BR-R-007** — Each decoded request received upstream is forwarded downstream unmodified (same unit id, function code, address, count) and awaited; the downstream response or exception is relayed back upstream unmodified. Bridge mode imposes no register/bit-count limit beyond each transport's own wire format — pure pass-through, no bridge-side cap.

**BR-R-008** — A request addressed to slave id 0 received on an RTU upstream interface is forwarded downstream and receives no response upstream, matching the existing RTU-server broadcast-silence rule (`MB-R-103`).

**BR-R-009** — A request forwarded to an RTU downstream interface addressed to slave id 0 is transmitted fire-and-forget (not awaited), matching the existing RTU-client broadcast-write rule (`MB-R-102`).

**BR-R-010** — When the downstream interface fails to connect while a forwarded request is outstanding (no established connection/serial link), bridge mode answers the upstream requester with exception `GatewayPathUnavailable` (0x0A). When downstream is connected but the forwarded request itself times out or the connection drops before a response arrives, bridge mode answers with exception `GatewayTargetDeviceFailedToRespond` (0x0B). Both codes are already present in the vendored `rust_modbus::ExceptionCode` enum (values 10/11); existing ferrowl servers never emit them today (`api-contract.md`'s exhaustive list is 0x01–0x04), since only bridge — not an ordinary client/server module — has a second link whose failure must be reported back to a first.

**BR-R-011** — TCP-socket interfaces (`tcp`, `rtu_over_tcp`, `ascii_over_tcp`; upstream and/or downstream) may enable TLS through descriptor keys that mirror the block form path for path, the interface's role being fixed by its flag rather than configured: an upstream descriptor carries a `ServerTlsPolicy` (BR-R-005), a downstream descriptor a `ClientTlsPolicy` (BR-R-006), and neither carries the two-role container of MB-R-104. Within BR-R-004's unchanged `key=val[,key=val]` grammar each TLS key is one dotted path — `tls.mode` (`none`|`tls`|`mutual`, default `none`), `tls.identity.source` (`ephemeral`|`self-signed`|`files`), `tls.identity.cert_file`, `tls.identity.key_file`, `tls.verification.verify` (`skip`|`root-store`|`ca-files`), `tls.verification.ca_files`, `tls.verification.extra_ca_files` — and the two CA-list keys take `;` as their intra-value delimiter, `,` already being the descriptor's key separator. A TLS key whose path the selected variant does not define shall be a setup failure (exit 1, BR-R-013), as shall `tls.verification.verify=root-store` on an upstream descriptor and `tls.identity.source=ephemeral` on a downstream one — the role-only halves of MB-R-105's three remaining checks. Resolution, verification, and validation otherwise follow `MB-R-104–109`; the bridge defines no TLS field of its own.

```
--upstream 'transport=tcp,ip=0.0.0.0,port=8502,tls.mode=mutual,tls.identity.source=self-signed,tls.verification.verify=ca-files,tls.verification.ca_files=/etc/ferrowl/a.pem;/etc/ferrowl/b.pem'
```

## Logging and process contract

**BR-R-012** — Bridge mode drains relayed-request and lifecycle log lines to stdout using the existing `[<timestamp>] <source> | <message>` format (`CL-R-040`), optionally appends to `--log-file` (`CL-R-041` semantics: create-and-append), and keeps setup/fatal diagnostics on stderr (`CL-R-042`).

**BR-R-013** — Exit codes mirror `run`'s scheme (`CL-R-030–032`): exit 1 for setup failure (missing/invalid `--upstream`/`--downstream`, upstream bind/listen/serial-open failure); exit 0 on `--duration` deadline or Ctrl-C; with `--exit-on-error` set, exit 3 on a drained `[bridge]`-sourced error line.

**BR-R-014** — `bridge` accepts an optional `--duration <secs>` flag, identical semantics to `run`'s (`CL-R-013` family).

## Multidrop bus safety

**BR-R-015** — The upstream descriptor's optional `unit_ids` key takes a comma-separated list/range (e.g. `unit_ids=1,3,5-8`) of allowed unit ids. When present, only a request addressed to a listed unit id is forwarded downstream (`BR-R-007`); a request for any other unit id is ignored entirely — not forwarded, no upstream response sent — so another device sharing the upstream link can answer it directly. When absent, every received unit id is forwarded (matching `MB-R-065`'s existing no-filter default), appropriate for a dedicated point-to-point link such as TCP-TCP where no other device shares the wire.
