# spec-reviewer — plan scope

Read by `spec-reviewer` only in scope `plan` or `plan from s<n>` (its `## Plan scope`); everything in `.claude/agents/spec-reviewer.md` still applies.

## Plan checks

Check `plan.md` against `spec-diff.md`, spec-fidelity axis only:
- quoted requirement text matches
- appended IDs unused in `docs/specs/`
- nothing contradicts an existing requirement
- every ID has a stage and a test
- `s0` lands every ID (no `s0` when `spec-diff.md` has no entries to land, or there is no `spec-diff.md`)
- no local `R<n>`/`E<n>` left; every fixed ID new to the run is unused on `main` (`git grep -n <ID> main -- docs/specs`) and sequential within its prefix
- every scenario `S<n>` is pinned by a test or a stage's Verification
- the plan reaches `architecture.md`: every new or changed component has a stage, no stage builds one it doesn't name, and every signature in `## Interfaces` appears as written in the step that builds it
- spec edits the planner made since the architecture stop are recorded (`Revisions:`, a `decisions.md` line each)
- every stage's `files` list covers what its steps touch and matches `## Shared`
- every stage title obeys `sh .claude/scripts/extract-section.sh '## Stage title' .claude/agents/spec-planner.md` and matches its heading and tree entry

## Re-plan checks

`plan from s<n>` — a re-plan after code landed: `diff artifacts/<slug>/plan.prev.md artifacts/<slug>/plan.md` (no `plan.prev.md`, a rebuilt board: read `plan.md` whole and the committed stages' cards instead of a diff). Any change to a section before `s<n>`, to `spec-diff.md` or a fixed ID is a blocker. Every rewritten stage gets the checks above that concern stages; every ID and scenario a replaced stage pinned is pinned by a rewritten stage or a committed one; a step that changes the spec names the entry it changes.
