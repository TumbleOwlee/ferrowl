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

`--exit-on-error` uses exit code 3, distinct from the clap usage-error code
(2), mirroring `run`'s scheme (see `cli-headless/edge-cases.md`).

- **`;` inside a descriptor value** — a multi-file CA list is the one descriptor
  value that carries its own delimiter, because `,` is already spent separating
  keys and the merged `CertVerification` genuinely takes a list where the retired
  `ca_file`/`client_ca_file` took one path. A path containing a literal `;` is
  therefore unreachable through a descriptor; bridge mode is a CLI-only relay
  (BR-R-002) with no config file to fall back on, and the alternative — a repeated
  key — would break BR-R-004's one-key-one-value grammar for every other key too.
- **`tls.*` on an `rtu` descriptor** — rejected outright (exit 1) rather than ignored, even `tls.mode=none`, which would be a no-op if honoured. A serial link has no TLS layer to configure, so the only thing such a key can express is a mistake about which of the two descriptors is being written; failing at setup says so at the one moment it is cheap to fix, where silent acceptance would leave the operator believing a plaintext bus was protected (BR-R-011).
