# Bridge Mode — Edge Cases and Known Limitations

Boundary behavior, error semantics, and the constraints that are
**intentional**. Everything here is working as implemented; it is recorded so
it is not mistaken for an oversight and silently "fixed".

---

## 1. Link loss and reconnect

| Condition | Behavior |
|---|---|
| Upstream RTU serial link is lost | ends the bridge task with an error; there is no reconnect for the upstream side (mirrors `modbus/edge-cases.md` — no server-side reconnect exists anywhere in the codebase today) |
| Downstream connection/link is lost or unavailable | follows the existing 1s–30s backoff reconnect while upstream keeps accepting/serving; a request arriving upstream during downstream backoff gets the `BR-R-010` exception rather than blocking indefinitely |
| Downstream connect fails at bridge startup | not a setup failure — the process starts normally and every forwarded request is answered `GatewayPathUnavailable` until downstream connects |
| Downstream descriptor has `reconnect` unset (`false`) and a connect/exchange failure occurs | never retries; every subsequent forwarded request answers `GatewayPathUnavailable` indefinitely |

## 2. Pass-through and filtering

| Condition | Behavior |
|---|---|
| A request's register/bit count | no per-request register-count/PDU-size enforcement beyond each transport's own wire format (mirrors `modbus/edge-cases.md` §6.1) — pure pass-through, no bridge-side cap |
| An RTU descriptor's `slave` key, if given | inert for bridge exactly as it is for an ordinary RTU server today (mirrors `modbus/edge-cases.md` §6.3): bridge answers whichever slave ids arrive since it owns no store to filter by |
| A downstream descriptor's `unit_ids` key, if given | parsed (the grammar is shared with upstream) but never consulted — `unit_ids` filtering (`BR-R-015`) is upstream-only |

## 3. Multidrop bus safety

On a shared/multidrop RTU upstream bus, `unit_ids` (`BR-R-015`) is how the
bridge avoids colliding with other real devices on the same wire: an
unfiltered bridge would forward a request meant for another device
downstream, get it rejected/timed out there, and answer upstream with a
failure that collides on the bus with the real device's own independently-sent
correct answer — omitting `unit_ids` is only safe on a dedicated
point-to-point upstream link.

## 4. Exit codes

Exit code 2 remains overloaded with the clap usage-error code, the same known
limitation `run` already has (see `cli-headless/edge-cases.md`) — not solved
for bridge, just inherited.
