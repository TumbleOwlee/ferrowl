# CLI & Headless — Edge Cases & Known Limitations

Boundary and error behavior of the process command line and the headless runner,
plus the stated known-limitations this area owns. Config-file and `migrate`
transformation edge cases live in [`../config-session/`](../config-session/);
Lua/assertion semantics live in [`../scripting/`](../scripting/).

---

## 1. Argument-parsing errors

- **Unknown flag / missing required option** — the argument parser aborts before any
  run or migration, prints a usage diagnostic to stderr, and exits with its standard
  usage exit code (**2**). No modules are started.
- **`--help` / `--version`** — printed to stdout, process exits **0**, no run occurs.
- **Exit-code-2 ambiguity for CI** — a parser usage error and an
  `--exit-on-error` assertion trip both exit with the integer **2**, from different
  stages (parse-time vs run-time). A CI script that distinguishes "the test failed"
  from "I mistyped a flag" cannot do so on the exit code alone; it must also inspect
  stderr (usage text vs a `[sim]` line on stdout). Stated limitation, not a bug.

## 2. Malformed `--module` / `--ocpp` descriptors

- **Segment without `=`** (e.g. `name=m,oops,device=d`) → parse error.
- **Empty comma segment** (e.g. `name=m,,device=d`) → skipped, not an error.
- **Missing required key** — `--module` without `name`, or without both `device` and
  `type`; a TCP `--module` without `port`; an RTU `--module` without `path`; an
  `--ocpp` without `name`, `device`, or `port` → parse error.
- **Non-numeric `port`/`data_bits`/`stop_bits`/`baud`** → parse error.
- **Invalid enum value** — `role` other than `client`/`server`, `transport` other
  than `tcp`/`rtu`, `protocol` other than `ws`/`wss` → parse error.
- In `ferrowl run`, any such descriptor error is a setup failure: the run exits
  **1** with an `Error:` diagnostic on stderr before the loop starts. In the TUI
  path it aborts startup with `Error:` on stderr and no TUI.

## 3. `run` with no modules / no session

- **`ferrowl run` with no `--module`, `--ocpp`, or `--session`** — resolves an empty
  module set, creates no session sim, and (without `--duration`) idles until Ctrl-C,
  then exits **0**. With `--duration N` it exits 0 after N seconds having produced no
  drained log lines. This is a valid, if useless, invocation — it is not rejected.
- **`--session` file present but carrying no enabled session script** — no session
  sim is created; only per-module logs (if any) are drained. Zero *enabled* scripts
  is treated the same as zero scripts.

## 4. `--duration` boundaries

- **`--duration 0`** — the deadline is computed as "now", but it is only checked at
  the **end** of the first tick, so the run executes roughly one ~100 ms tick (one
  refresh + drain) and then exits **0**. It is not a zero-time no-op; it is a
  one-tick run. Stated behavior.
- **Very large `--duration`** — accepted as given (seconds); there is no upper clamp.
- Ctrl-C always short-circuits `--duration` and exits **0**.

## 5. Session / device load failures in headless

- **A `--session` file that fails to load or parse** — the runner treats it as a
  setup failure and exits **1** (`Error:` on stderr). This is stricter than the TUI,
  which validates loadability up front but tolerates a file vanishing mid-startup by
  falling back to defaults.
- **A module's device config that fails to load** — headless exits **1**
  (`'<name>': failed to load '<path>': …` under `Error:`). The TUI instead skips that
  one module with a stderr warning and keeps the rest (see
  [`../config-session/`](../config-session/) §3). This asymmetry is deliberate:
  headless must not silently run a partial module set in CI.
- **A blank `device` path** — for OCPP this is a legitimate quick-start on the default
  device config (not a failure); the envelope rule (CS-R-053) governs it.

## 6. `--exit-on-error` detection is prefix-based, not level-based

- Detection keys off the literal line prefix `[sim]`, matched on the drained
  message string, **regardless of the line's log level**. It is plain log-string
  detection, not a structured error channel: it catches only errors that are actually
  *logged* under that prefix, and it would also trip on any non-error line that
  happens to start with `[sim]`. A Lua error that never reaches the log (or is logged
  without the prefix) will not trip it. Stated limitation — the contract is exactly
  "a drained line starts with `[sim]`", nothing more semantic.
- Consequence for assertions: `C_Test:Assert` failures surface through the sim's
  `[sim] <error>` log line. Without `--exit-on-error`, an assertion failure does
  **not** change the exit code — the run still exits 0. CI that must fail on assertion
  failure must pass `--exit-on-error`. (Assertion semantics: see
  [`../scripting/`](../scripting/).)

## 7. `--log-file`

- **Unopenable path** (bad directory, permission denied) — the runner stops any
  already-started modules and exits **1** with `Error: failed to open --log-file …`.
- **Existing file** — opened create-and-append: prior content is preserved and new
  lines are appended, not truncated. Two runs targeting the same file accumulate.

## 8. Log draining under load

- Draining is exact-by-count from a monotonically increasing written-line counter, so
  a message repeated verbatim within one window is fully emitted (not de-duplicated,
  not mis-resumed).
- If more lines are written between ticks than the bounded log ring holds
  (~80 lines), the oldest overflow lines are gone; the runner emits a synthetic
  `(<n> lines dropped: ring overflowed between ticks)` line rather than silently
  under-counting. A sim that logs faster than one tick can drain can therefore lose
  the *content* of the oldest lines while still accounting for their count.

## 9. Top-level flags alongside a subcommand

- Passing top-level `--module`/`--session`/`--device`/`--demo` together with a `run`
  or `migrate` subcommand is accepted by the parser but has **no effect**: the
  subcommand reads only its own flags. `ferrowl --module X run --duration 1` runs a
  headless session with **no** modules (the top-level `--module X` is silently
  ignored). Easy to trip over; stated behavior.

---

## Known limitations (stated, not bugs)

- **The RTU `Config` / clap short-flag collision.** The Modbus RTU settings struct in
  the `ferrowl-modbus` crate doubles as a clap `Args` group with auto-derived short
  flags. Two short options collide: `-s` is claimed by both `slave` and `stop_bits`,
  and `-d` by both `data_bits` and `delay_ms`. Flattening this `Args` group into a
  `clap::Parser` command would panic at parse time via clap's debug assertions. This
  is a **latent** defect: ferrowl's production CLI does **not** flatten that struct —
  RTU parameters are supplied through the `--module …,transport=rtu,path=…,baud=…`
  key=val mini-language ([`api-contract.md`](./api-contract.md) §3), which has its own
  key namespace and no short-flag collision. So the collision cannot be triggered
  through ferrowl's shipped command line today; it would only bite if someone wired
  that `Args` group directly into a `Parser`. The collision is documented in the
  source but intentionally left unfixed. (An equivalent latent collision exists in the
  TCP settings `Args` group for the shared `timeout`/`delay`/`interval` short flags;
  same story — not reachable through the shipped CLI.)

- **`migrate`'s `-i`/`-o` short flags are the only short options on ferrowl's own
  command line.** The top-level flags (`--module`, `--session`, `--device`, `--demo`)
  and every `run` flag are long-only. This is intentional; there is no `-m`/`-s`/`-d`
  short form for the module/session/device flags (which sidesteps any short-flag
  collision of the kind above).

- **Exit code 2 is overloaded.** As noted in §1, the parser's usage-error code and the
  `--exit-on-error` assertion-failure code are both 2. There is no distinct code for
  "bad invocation" vs "test assertion tripped".

- **`--exit-on-error` only catches logged, prefixed errors.** Per §6, it is a string
  match on `[sim]`, not a structured result channel. Errors outside the sim log, or
  logged without the prefix, are invisible to it.

- **Headless has no per-module error isolation.** Any one module's startup failure
  fails the whole `run` with exit 1 (§5); there is no "start the good ones, report the
  bad one" mode in headless, by design.
