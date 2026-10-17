# Decisions — one file per run

Read by `spec-author`, `spec-architect`, `spec-planner` and `spec-implementer` before recording a decision; `spec-reviewer` reads the file, never writes it.

Every decision of the run, asked or settled alone, is one line appended to `artifacts/<slug>/decisions.md` (absolute path from your brief; create it with the heading `# <slug> — decisions` when absent). Append only (`>>`), never edit or delete a line: a decision reversed later is a new line saying so, and the later line wins.

```
- <choice> (<user | chosen>) [<scope | spec <ID> | design | process>] — <≤ 1 line why> · <author | architect | planner | implementer>
```

- `(user)` — the user answered it; the why may be dropped. `(chosen)` — you settled it alone; the why is mandatory. Every `(chosen)` line the user has not yet seen is quoted under the next stop's `Confirm:`; `(user)` lines never are.
- `[scope]` — what is in or out, a scenario's flow. `[spec <ID>]` — fixes or changes observable behavior of that entry (local ID before gate 2, fixed after). `[design]` — a structure or code choice a PR reader needs. `[process]` — stage cut, titles, tests, workflow.
- Never a stage id or an agent name in the choice or the why — the trailing `· <agent>` is the only one; `pr-body.sh` renders the `[design]` lines and the `(chosen)` `[spec]` lines not already under the PR's spec changes.
- A change that contradicts a `(user)` line is asked first, never recorded.
