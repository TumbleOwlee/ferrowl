# Bridge Mode — Edge Cases and Known Limitations

Boundary behavior, error semantics, intentional constraints. Working as implemented; recorded so it is not "fixed".

---

## 1. Link loss and reconnect

| ID | Condition | Behavior |
|---|---|---|
| **BR-E-001** | Upstream RTU serial link lost | ends the bridge task with an error; no upstream reconnect (mirrors `modbus/edge-cases.md` MB-E-076 — no server-side reconnect exists anywhere today) |
| **BR-E-002** | Downstream connection/link lost or unavailable | existing 1s–30s backoff reconnect while upstream keeps accepting/serving; a request arriving during downstream backoff gets the `BR-R-010` exception rather than blocking |
| **BR-E-003** | Downstream connect fails at startup | not a setup failure — process starts normally, every forwarded request answered `GatewayPathUnavailable` until downstream connects (`BR-R-010`) |
| **BR-E-004** | Downstream `reconnect` unset (`false`) and a connect/exchange failure occurs | never retries; every subsequent forwarded request answers `GatewayPathUnavailable` indefinitely (`BR-R-006`, `BR-R-010`) |

## 2. Pass-through and filtering

| ID | Condition | Behavior |
|---|---|---|
| **BR-E-005** | A request's register/bit count | no per-request count/PDU-size enforcement beyond each transport's wire format (mirrors `modbus/edge-cases.md` MB-E-073; `BR-R-007`) — pure pass-through |
| **BR-E-006** | RTU descriptor's `slave` key, if given | inert, as for an ordinary RTU server (mirrors `modbus/edge-cases.md` MB-E-075): bridge answers whichever slave ids arrive, owning no store to filter by |
| **BR-E-007** | Downstream descriptor's `unit_ids` key, if given | parsed (shared grammar) but never consulted — `unit_ids` filtering (`BR-R-015`) is upstream-only |

## 3. Multidrop bus safety

On a shared/multidrop RTU upstream bus, `unit_ids` (`BR-R-015`) keeps the bridge from colliding with other devices on the wire: an unfiltered bridge would forward a request meant for another device downstream, get it rejected/timed out, and answer upstream with a failure colliding on the bus with the real device's own correct answer — omitting `unit_ids` is safe only on a dedicated point-to-point upstream link.

## 4. Exit codes

`--exit-on-error` uses exit 3, distinct from the clap usage-error code 2, mirroring `run` (`cli-headless/edge-cases.md` CL-E-003).

- **BR-E-008** — **`;` inside a descriptor value** — a multi-file CA list is the one descriptor value carrying its own delimiter, because `,` separates keys and the merged `CertVerification` takes a list where the retired `ca_file`/`client_ca_file` took one path. A path containing a literal `;` is unreachable through a descriptor; bridge is CLI-only (BR-R-002) with no config file to fall back on, and a repeated key would break BR-R-004's one-key-one-value grammar for every other key.
- **BR-E-009** — **`tls.*` on an `rtu` descriptor** — rejected outright (exit 1) rather than ignored, even `tls.mode=none`, which would be a no-op if honoured. A serial link has no TLS layer, so such a key can only express a mistake about which descriptor is being written; failing at setup says so when it is cheap to fix, where silent acceptance would leave the operator believing a plaintext bus was protected (BR-R-011).
