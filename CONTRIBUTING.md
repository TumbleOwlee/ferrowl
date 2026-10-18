# Contributing to Ferrowl

One page: how to build and test, the rules every change follows, and what you see and answer when an agent drives the change for you. The rulebook behind it ships inside [ferrule](https://github.com/TumbleOwlee/ferrule) and is printed step by step with `ferrule guide`; you never need to read it to work with it.

## Setup

Rust stable, pinned by `rust-toolchain.toml` ([rustup.rs](https://rustup.rs/)):

```sh
cargo build --release
cargo run --release -- --demo          # built-in demo tabs, no config
cargo install cargo-llvm-cov --locked  # for the coverage gate
cargo install --git https://github.com/TumbleOwlee/ferrule --tag v0.1.0 --locked  # the workflow's CLI, pinned in .ferrule/ferrule.toml
lefthook install                       # fmt and clippy before every commit; no commit or push to main
```

Crate graph and responsibilities: [`ARCHITECTURE.md`](./ARCHITECTURE.md). Product framing: [`PRD.md`](./PRD.md).

## The rules

- **The spec is authoritative.** [`docs/specs/`](./docs/specs/) says what the software must do; code conforms to it. A behavior change ships its spec change in the same PR. New entries get a fresh, appended ID, never renumbered or reused (`docs/specs/README.md` rule 2).
- **Test first.** Write the test, watch it fail, implement. Expected values come from the protocol or upstream API, never from your own implementation's output. Every new or changed requirement gets a test whose doc comment cites its ID (`/// MB-R-012 — …`, several IDs comma-separated; `docs/specs/README.md` rule 8). Unit tests: `#[cfg(test)] mod tests`, `ut_*`. Integration tests: the crate's `tests/`, `it_*`.
- **Coverage ≥ 80% of lines**, CI-gated. A floor, never padded with tests that assert nothing.
- **Before you push**, all of these pass (CI runs them on every push and every PR):

```sh
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo llvm-cov --workspace --fail-under-lines 80
ferrule install --check           # when .ferrule/ or a generated fe-* file changed
```

- **Pull requests**: branch `<type>/<slug>` off `main` (`feat/`, `fix/`, `docs/`), one change per PR, requirement IDs in the body, README updated for user-facing commands, keys, config or the Lua API. Squash-merged. No tool attribution (`Co-Authored-By` for an assistant, "Generated with") anywhere.

## Working with the agents

You talk to one agent, the orchestrator. It spawns the others (author, architect, planner, implementer, reviewer), relays their questions, runs git and GitHub, and stops whenever a decision is yours. Everything else runs without you.

**Start**

| You want to | Type |
|---|---|
| Build a feature or fix a bug | `/fe-feature <goal>` or `/fe-feature take issue #42` |
| Turn a requirement into an approved ticket, nothing more | `/fe-request <requirement>` |
| Review someone else's finished PR against its ticket | `/fe-review #<issue>` (the ticket; its PR is found from it) |
| Carry on after a crash or a closed session | `/fe-feature` again — it reconciles the board with git and resumes |

**Lanes.** At the first stop you pick a lane; the orchestrator recommends one by size.

| Lane | Recommended for | Stops you see |
|---|---|---|
| fast | up to three entries, or a refactor touching up to three components | architecture (with the brief) · PR; at most one entry: plan (with the brief) · PR |
| full | anything bigger | brief · architecture · plan · stages that need you · PR (with the branch review) |

A feature too big for one run (many scenarios, several areas) becomes an **epic**: one run that settles the shared core and files one issue per slice, each slice a run of its own. A non-behavior change (refactor, rename, tooling) starts at the architecture step; a typo needs no run at all, just a branch and a PR.

**Stops.** Every stop is one message: a header (`Gate 2 · <slug> · plan.summary.md rev 2`), the lines that matter from the files it opened in your viewer, `Approving will:` and how to answer. Two shapes only:

```
Approving will:                                Question · <slug> · gate 1 · fe-author · 2 decisions
- file the issue                               ## 1. Retry forever or give up after an hour?
- create worktree … on branch feat/x           Options:
Options:                                       - a) retry forever (recommended: …)
1. switch to the fast lane                     - b) give up after an hour
Answer: approve [<n>[: <value>] …]             ## 2. …
        · change <what> · stop                 Answer: <1: a · 2: b · …> · other <text>
```

- `approve` lets the run continue; `approve 1` also takes option 1; `approve 1: s1,s3` gives the option a value.
- `change <what>` goes back to the agent that wrote the file; you see the stop again with the revision.
- `stop` halts with everything kept — resume later with `/fe-feature`. At the PR stop it holds for a manual run.
- A question batch is answered by number and letter (`1: a · 2: b`), or `other <text>`.
- A `Needs user` item under an approval (a reviewer's question to you) is answered in the same reply by its letter: `approve · a`.
- `re-plan: <why>` at any stage stop or after an auto-continue line rewrites the stages not yet committed.
- `Confirm:` lists what agents decided on their own since your last approval. Approving accepts them; `change` any you disagree with.

**Notices** need no answer: `Landed s2 · … · next: s3 (auto-continue — say stop to hold)`. Any message from you is read before the next stage starts; `stop` holds at the next green.

**Reviewing as you go.** The first pushed stage opens a draft PR. Its body is the live board: stages, requirements, spec changes, architecture, decisions. Leave inline comments on the diff; they are fetched before every stop, fixed by the implementer and answered on the thread (`fixed`, or `declined` with the reason, left open for you). Resolve a thread yourself to drop it. The run ends when the PR is marked ready and CI is green; you review and merge on GitHub.

**When the spec changes mid-run.** The brief is best effort and settles as the run learns. A *detail* change (a deferred point filled, wording sharpened) is listed and posted to the issue as a changelog comment. A *meaningful* one — it contradicts a scenario, an answer of yours or the approved architecture — comes to you: keep, revert, or split into its own issue.

**Where things are.** `.ferrule/state.db` is the board: cards, their logs, one metrics line per finished run, the archive of finished runs (`ferrule card board`, `ferrule card show <id>`, `ferrule run metrics`). `.ferrule/runs/<slug>/` holds every file the agents wrote (`spec-diff.md` the brief, `architecture.md`, `plan.md`, `decisions.md`, `review.md`). Local, never committed, never edited by hand. Each run works in its own worktree under `.ferrule/worktrees/`.

## Changing the workflow itself

The workflow's mechanics and its instructions live in [ferrule](https://github.com/TumbleOwlee/ferrule): commands with their contract in `--help`, instructions as `ferrule guide` topics. Changes to either go there. What is ferrowl's own sits here: `.ferrule/ferrule.toml` (gauntlet steps, thresholds) and `.ferrule/guide/<topic>.md`, which overrides a topic section by section. The `fe-*` agents and skills under `.claude/` are generated by `ferrule install` — never edit them; `ferrule install --check` in CI catches drift. Moving to a new ferrule version is `ferrule install` with that binary, committed with the `version` it writes. A change that adds stops or agents names its expected effect on `ferrule run metrics` in its PR.

## Reporting issues

Open a GitHub issue with steps to reproduce, the ferrowl version (or commit) and your platform. For TUI rendering issues, the terminal emulator and size help too.
