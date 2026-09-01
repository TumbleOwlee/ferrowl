# AGENTS.md

Router for AI coding agents. Read first; points to everything else.

## Repo

Ferrowl — Rust TUI simulator for Modbus (client/server, TCP/RTU) and OCPP (Charging Station/CSMS, 1.6/2.0.1/2.1). Cargo workspace, 14 crates, one `ferrowl` binary. Product: [`PRD.md`](./PRD.md). Structure: [`ARCHITECTURE.md`](./ARCHITECTURE.md).

## Spec-driven

- `docs/specs/` authoritative. Code conforms to spec, never reverse.
- Read area's `requirements.md` + `edge-cases.md` before editing that area. `edge-cases.md` = deliberate ugliness; check before "fixing".
- Behavior change with no spec change = incomplete.
- `main` never holds unfinished spec: a requirement on `main` describes code that exists and is tested. A branch may hold a spec commit ahead of its code; squash merge keeps it off `main`.
- Pre-existing spec/code disagreement outside your task: stop, raise separately. Folding it in widens approved work and skips its own review.
- Specs carry no `file:line`. Locate code with search tools.
- Requirement IDs stable, append-only. Cite in commits and PRs.
- One requirement, one physical line, never wrapped. Find by `grep -rn <ID or keyword> docs/specs/`; exact file:line: `sh .claude/scripts/extract-id.sh <ID> [<ID> ...]` (batch IDs into one call). One section of a large file: `sh .claude/scripts/extract-section.sh '## <heading>' path/to/file.md`.

## TDD — fixed order, every stage

1. Write the test. Doc comment cites requirement ID (`/// MB-R-012 — …`).
2. Run it, watch it fail for the right reason, report the failure. Wrong assertion / test-side compile error / premature pass proves nothing.
3. Minimum implementation that passes.
4. Refactor green.

- Implementation without a preceding failing test: not done. Test written after the fact to fit code: not done.
- Expected values from the authoritative source (standard/protocol/upstream API), never a debug print of your own implementation.
- Coverage floor 80% of lines, CI-gated. A floor, not a target: never inflate with tests that execute code without asserting.

## Workflow

Triggers on **behavior change, any size**: new public function, changed default, new error variant, any observable semantics. Size sets stage count, never gate existence. Non-behavior change (refactor, rename, perf with identical semantics, test-only, docs, tooling): gate 1 skipped, rest runs. Trivial edit (one file, no semantics, no test/CI/build effect — typo, comment, doc wording): no gates, branch + PR.

Gate/task-board mechanics (Gate 1 through Merge, Resume): [`.claude/AGENTS.workflow.md`](./.claude/AGENTS.workflow.md). Pull one section at a time with `extract-section.sh`.

## Where to look for task X

| Task touches | Read | ID prefix |
|---|---|---|
| Modbus register codec, store, client/server (TCP/RTU) | [`docs/specs/modbus/`](./docs/specs/modbus/) | `MB-R-*` |
| OCPP actions, CS/CSMS engine, versions 1.6/2.0.1/2.1, TLS/auth | [`docs/specs/ocpp/`](./docs/specs/ocpp/) | `OC-R-*` |
| Lua scripting (`C_*` API, sim threads, sandbox) | [`docs/specs/scripting/`](./docs/specs/scripting/) | `SC-R-*` |
| TUI widgets, dialogs, `:` commands, keybindings, code editor | [`docs/specs/tui/`](./docs/specs/tui/) | `UI-R-*` |
| Config/session file format, save/load, `migrate` | [`docs/specs/config-session/`](./docs/specs/config-session/) | `CS-R-*` |
| CLI flags, `ferrowl run` headless, exit codes | [`docs/specs/cli-headless/`](./docs/specs/cli-headless/) | `CL-R-*` |
| Bridge mode: downstream/upstream Modbus relay (TCP/RTU) | [`docs/specs/bridge/`](./docs/specs/bridge/) | `BR-R-*` |
| Platforms, performance, security, versioning, testing conventions | [`docs/specs/non-functional-requirements.md`](./docs/specs/non-functional-requirements.md) | `NF-R-*` |
| Crate graph, data flow, concurrency model | [`ARCHITECTURE.md`](./ARCHITECTURE.md) | — |
| Contribution workflow, conventions | [`CONTRIBUTING.md`](./CONTRIBUTING.md) | — |

Each area's `edge-cases.md` records **known limitations** — ugly but intentional. Check before "fixing".

## Build / test / lint

```sh
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo check --workspace
cargo test --workspace
cargo llvm-cov --workspace --fail-under-lines 80
```

Narrow the loop while iterating. `cargo test --workspace <name>` still builds and runs every crate's test binary (each prints `test result: ok. 0 passed`), even though only one crate can match:

```sh
cargo test -p ferrowl-modbus              # one crate
cargo test -p ferrowl-modbus <name>       # one test in one crate — not `cargo test --workspace <name>`
cargo test -p ferrowl-codec ut_decode     # one test (unit tests are named ut_*)
cargo check -p ferrowl-ocpp               # typecheck one crate
cargo llvm-cov --workspace --html         # browsable per-line coverage
```

Full set before done. `lefthook` enforces `fmt --check` and `clippy -D warnings` pre-commit; CI runs the full set plus `cargo deny check` on every push **and every pull request**.

Dev loop: `cargo run --release -- --demo` (built-in demo tabs, no config) or `cargo build --profile fastrel` (opt-level 1, faster iterative builds).

## Conventions — reading

- **Never read a whole file when only part is needed.** Any `.md` (specs, `SKILL.md`s, other repos' docs): `sh .claude/scripts/extract-section.sh '<heading>' ['<heading>' ...] <file>` (unknown heading: `sh .claude/scripts/list-sections.sh <file>` first). Other large files: `sed -n '<start>,<end>p' <file>`. Applies to Read tool and Bash `cat` alike — same context cost. **Enforced:** `PreToolUse` hook (`.claude/scripts/hook-guard-shell.sh`) denies an unpiped Bash `cat` of a `.md` file or any file over 80 lines, pointing at `extract-section.sh`/`sed -n`/Read. A denial = convention about to be bypassed; follow the redirect, don't retry the `cat` differently.
- **Filter shell output before it lands in context.** `find -name`/`-path`, `git show --stat` or a path filter before full content, `grep`/`tail -N`/`head -N` on `cargo test`/`cargo llvm-cov` output.
- **Don't re-run a read-only command whose output is already in context** (`git diff`, `git log`, `git show` on the same refs/paths). Scroll back.

## Conventions — code

- Unit tests: `#[cfg(test)] mod tests` at the bottom of the file under test, names `ut_*`. Integration tests: crate's `tests/`, names `it_*` (notably `ferrowl-ui` and much of `ferrowl`).
- **Tests get ports from `ferrowl-test-support`, never a bare probe.** `reserve_tcp_port()`/`reserve_udp_port()` bind `127.0.0.1:0` and hold the binding; the guard's socket is handed to the server via `into_listener()`/`into_socket()` where the server accepts a bound socket, and `release()` is used only where the server can bind by number alone — a bare `free_port()`-style probe-and-drop is forbidden. The port-occupier pattern (bind ephemerally, keep the binding alive, point the server at the same port so the collision is the assertion) stays exempt. Scratch files come from `reserve_temp_dir()`, never straight from `std::env::temp_dir()`.
- All 14 crates versioned in lockstep; never bump one alone.
- Config files TOML or JSON only (extension-driven), never YAML.
- Rust edition 2024, stable toolchain (`rust-toolchain.toml`).
- **Never split a source file just because it is large.** A split must separate distinct responsibilities, improve navigability, or cut coupling. One cohesive concern or flat generated data (spec table) stays whole. Line count = prompt to review, not mandate to divide; arbitrary boundaries make code harder to follow.
- **A comment says what the code cannot.** No restating the adjacent statement/field/function name; no step narration (`// Create app state`); no banners or import-group headers; no paragraph where a sentence does. **Never cite this workflow** — plan, stage id (`s7`), gate (`Gate3#2`), task item, `(Shared)`, "sanctioned change": rots when the plan is deleted, meaningless to a later reader. Keep the technical content, drop the citation. Requirement IDs are the only sanctioned cross-reference. An `#[allow(...)]` justification names the condition that lifts it, never the stage that will. Applies to `//` and `///`.
- **Typed handling over generic JSON.** Read request fields and build responses from the typed `rust_ocpp` structs/enums (`req.evse_id`, `Response201::Reset(...)`), never by indexing or hand-crafting `serde_json::Value` where a typed path exists — compiler catches a wrong field/missing field/bad enum, not the wire. Holds **even when it forces duplication**: distinct-but-similar types across 1.6/2.0.1/2.1 get duplicated typed code per version, never a shared untyped `Value`. **Only** sanctioned untyped JSON: the manual payload a user types into the action dialog, and version-independent plumbing that must inspect an arbitrary encoded action (e.g. a scope/EVSE guard spanning all actions) where no typed accessor exists.
- **Model states as enums, never a flag plus dependent optionals.** Fields meaningful only under some combination of booleans push validation into a resolve function and let the wire carry states the code must reject. One variant per state, holding exactly that state's fields, so invalid combinations cannot be constructed or deserialized. A tagged enum (`#[serde(tag = "…")]`) extends this to the wire and replaces hand-written `Serialize`/`Deserialize` shadow structs. A check no type can express (non-empty `Vec`) stays one condition on one variant, never a rule spanning fields. Applies to config/session schemas as much as in-memory types; a wire-shape change is a breaking configuration change and needs its own CS-R spec change.

## Conventions — text

- **No hard line wrap on anything posted externally** — issue bodies, PR bodies, PR/review comments. GitHub soft-wraps; an inserted `\n` mid-sentence renders as a real line break. Paragraphs as single lines; only headings, list items, code blocks get their own line.
- **Spec text never wrapped** — `docs/specs/` and any `spec-diff.md`: one requirement, one physical line, however long, so `grep` returns the whole requirement and `extract-id.sh` points at one line. **Commit messages are the exception: wrap** — subject ≤ 72 columns, body at 72. `git log` never soft-wraps.

## Scope boundaries — check with the user before

- **Expanding the Lua `C_*` API.** Surface (`C_Register`, `C_OCPP`, `C_Time`, `C_Log`, `C_Test`, `C_Module`, `C_Statics`) deliberately small and fixed. A new module or method is a design decision.
- **Bridging Modbus and OCPP.** Architecturally separate; no shared lifecycle abstraction. A fix/pattern in one does not transfer without checking both specs.
