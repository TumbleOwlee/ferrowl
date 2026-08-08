Excerpt of AGENTS.md — spec-driven core, TDD order, build/test/lint, conventions, scope boundaries. Full gates and task board: ../AGENTS.md. Regenerate by re-copying these sections if they change.

## Spec-driven

- `docs/specs/` authoritative. Code conforms to spec, never reverse.
- Read area's `requirements.md` + `edge-cases.md` before editing that area.
  `edge-cases.md` = deliberate ugliness; check before "fixing."
- Behavior change with no spec change = incomplete.
- `main` never holds unfinished spec: a requirement on `main` describes code
  that exists and is tested. A branch may hold a spec commit ahead of its
  code; squash merge keeps that off `main`.
- Pre-existing spec/code disagreement outside your task: stop, raise
  separately. Folding it in widens approved work, skips its own review.
- Specs carry no `file:line`. Locate code with search tools.
- Requirement IDs stable, append-only. Cite in commits and PRs.
- One requirement, one physical line, never wrapped — find any by `grep -rn <ID or keyword> docs/specs/`, or with the exact file:line to edit: `sh .claude/scripts/extract-id.sh <ID> [<ID> ...]` (batch every ID needed into one call). Read one section of a large spec file instead of the whole thing: `sh .claude/scripts/extract-section.sh '## <heading>' path/to/file.md`.

## TDD — fixed order, every stage

1. Write the test. Doc comment cites requirement ID (`/// MB-R-012 — …`).
2. Run it, watch it fail for the right reason, report the failure. Wrong
   assertion / test-side compile error / premature pass proves nothing.
3. Minimum implementation that passes.
4. Refactor green.

- Implementation without a preceding failing test: not done. Test written
  after the fact to fit code: not done.
- Expected values from the authoritative source (standard/protocol/upstream
  API) — never a debug print of your own implementation.
- Coverage floor 80% of lines, CI-gated. A floor, not a target — never
  inflate it with tests that execute code without asserting.

## Build / test / lint

```sh
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo check --workspace
cargo test --workspace
cargo llvm-cov --workspace --fail-under-lines 80
```

Narrow the loop while iterating — don't run the whole workspace for one test:

```sh
cargo test -p ferrowl-modbus              # one crate
cargo test -p ferrowl-codec ut_decode     # one test (unit tests are named ut_*)
cargo check -p ferrowl-ocpp               # typecheck one crate
cargo llvm-cov --workspace --html         # browsable per-line coverage
```

Full set before done. `lefthook` enforces `fmt --check` and `clippy -D
warnings` pre-commit; CI runs the full set (plus `cargo deny check`) on every
push **and every pull request**.

Dev loop: `cargo run --release -- --demo` (built-in demo tabs, no config
needed) or `cargo build --profile fastrel` for faster iterative builds
(opt-level 1).

## Conventions

- **Never read a whole file when only part of it is needed.** For any
  markdown file (not just `docs/specs/` — skill `SKILL.md`s, other repos'
  docs, anything `.md`), use `sh .claude/scripts/extract-section.sh '<heading>'
  ['<heading>' ...] <file>` (unknown heading text: `sh
  .claude/scripts/list-sections.sh <file>` first) instead of `cat`/Read on the
  whole file. For any other large file where only a line range is needed, use
  `sed -n '<start>,<end>p' <file>` instead of a full `cat`/Read. This applies
  equally whether the read happens via the Read tool or a Bash `cat` — both
  cost the same context. **Enforced, not just advisory:** a `PreToolUse` hook
  (`.claude/scripts/hook-guard-cat.sh`) denies an unpiped Bash `cat` of a
  `.md` file, or of any file over 80 lines, with a message pointing at
  `extract-section.sh`/`sed -n`/the Read tool. A denial here means the
  convention was about to be bypassed, not a bug to route around — follow the
  message's redirect rather than retrying the same `cat` differently.
- **Filter shell output before it lands in context, not after.** `find`,
  `git show`, `git diff`, `cargo test`, `cargo llvm-cov` and similar can
  produce far more than is needed. Narrow at the shell — `find` with
  `-name`/`-path`, `git show --stat` (or a path filter) before full content,
  `grep`/`tail -N`/`head -N` on `cargo test`/`cargo llvm-cov` output — rather
  than dumping everything and reading past what's unneeded.
- **Don't re-run a read-only command for output already in context this
  session.** `git diff`, `git log`, `git show` etc. against the same
  refs/paths already shown once — scroll back instead of re-running.
- Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file under
  test, function names prefixed `ut_`. Integration tests belong in each
  crate's `tests/`, function names prefixed `it_` (notably in `ferrowl-ui`
  and much of `ferrowl`).
- **Tests bind ephemeral ports only.** Any test that starts a network
  listener (Modbus TCP, OCPP WebSocket, a raw occupier socket) binds port 0
  and reads the assigned port back — never a fixed port number. A fixed port
  makes the test fail whenever anything else on the machine holds it (a
  running `ferrowl --demo`, a parallel checkout's tests); ephemeral binding
  removes the collision class entirely. Fixed ports in specs that are never
  `start()`ed are inert, but prefer port 0 there too so the intent stays
  obvious. Deliberately *occupying* a port to test bind-failure handling
  still binds the occupier ephemerally first, then points the server at that
  port (see `tcp_loopback.rs`).
- All 13 workspace crates are versioned in lockstep. Don't bump one
  independently.
- Config files are TOML or JSON only (extension-driven), never YAML.
- Rust edition 2024, stable toolchain (`rust-toolchain.toml`).
- **Never split a source file just because it is large.** A split must earn
  its keep — it separates genuinely distinct responsibilities, improves
  navigability, or cuts coupling. A long file that covers one cohesive
  concern, or is flat generated data (e.g. a spec table), stays whole. Treat
  a line count as a prompt to *review* the file, not a mandate to divide it;
  an arbitrary boundary drawn to hit a number makes the code harder to
  follow, not easier.
- **Prefer typed handling over generic JSON.** Read request fields and build
  responses from the strongly-typed `rust_ocpp` structs and enums
  (`req.evse_id`, `Response201::Reset(...)`), never by indexing or
  hand-crafting a `serde_json::Value` where a typed path exists — the
  compiler must catch a wrong field name, missing field, or bad enum, not the
  wire. This holds **even when it forces duplication**: if two OCPP versions
  (1.6 / 2.0.1 / 2.1) have distinct-but-similar types, duplicate the typed
  code per version rather than collapse it onto a shared untyped `Value`.
  The duplication is the accepted price of compile-time typing. The **only**
  sanctioned untyped JSON is the manual payload a user types into the action
  dialog, and version-independent *plumbing* that must inspect an arbitrary
  encoded action (e.g. a scope/EVSE guard spanning all actions) where no
  typed accessor exists.
- **No hard line wrap on anything posted externally** — issue bodies, PR
  bodies, PR/review comments. The host (GitHub) soft-wraps for display; a
  manually inserted `\n` mid-sentence survives rendering as a real line
  break and fragments the text. Paragraphs as single unbroken lines; only
  headings, list items, and code blocks get their own line.

## Scope boundaries — check with the user before

- **Expanding the Lua `C_*` API.** The surface (`C_Register`, `C_OCPP`,
  `C_Time`, `C_Log`, `C_Test`, `C_Module`, `C_Statics`) is deliberately small
  and fixed. Adding a module or method is a design decision, not a
  mechanical addition.
- **Bridging Modbus and OCPP.** They are architecturally separate — no
  shared lifecycle abstraction spans both. Don't assume a fix/pattern in one
  applies to the other without checking both specs.
