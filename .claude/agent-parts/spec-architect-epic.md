# spec-architect — epics, slices, size guard

Read by `spec-architect` only on the triggers in its `## Epics, slices, size guard`; everything in `.claude/agents/spec-architect.md` still applies.

## Size guard

A plain behavior run whose architecture would add or change more than 8 components is too big for one run (a non-behavior run has no brief to slice: say so in `## Fit with existing code` and continue). Ask before writing it (`question.md`): a) turn it into an epic (recommended) — you continue in `## Epic mode` below on the same brief; b) continue as one run. A slice run past 8: a) split the slice — narrow it to its first part, the rest as `status=split`; b) continue.

## Epic mode

The epic run settles only what every slice builds on, then cuts the feature into slices. No slice-level detail.

`architecture.md` (shape above) covers the **skeleton**: the core abstractions and the seams between them — the traits, types and data model every slice extends — with full `## Interfaces` for those. Specific variants (one device type, one rule) are slice work: name them under `## Deferred` with their slice. `## Interactions` walks the scenarios at skeleton level. Grill the user on the skeleton exactly as on any architecture; the slices' order and cut are decisions too.

`slices.md`, one section per slice, in build order:

```
# <slug> — slices (rev N)

## Slice 1: <title, ≤ 4 words>
Goal: <what the user can do once it lands, one line>
Depends on: none | <slice numbers>
Scenarios: S1, S4
Entries: R1, R2, E1
Adds: <components and interfaces it lands or extends, one line>
Deferred: <the brief's Deferred items it settles, separated by `; `, or none>
```

`Depends on` takes bare slice numbers separated by `, `; `Scenarios` and `Entries` take the brief's IDs separated by `, `, or `none`.

- **Slice 1 is the walking skeleton**: it lands every interface of `## Interfaces` with the smallest working bodies and one scenario end to end.
- Every scenario, every `R`/`E` entry and every `## Deferred` item of the brief belongs to exactly one slice.
- Each slice fits one ordinary run: about 8 scenarios, 8 components and 5 stages at most; a slice above that is cut again.
- A slice is a vertical piece the user can try, never a layer ("all the types"); `Depends on` names earlier slices only.

Answer `status=ready file=… summary=… count=<skeleton components> slices=artifacts/<slug>/slices.md`.

## Slice run

`epic.md` is the epic issue's spec and skeleton architecture, followed by the epic's comments (skeleton changes earlier slices kept — they override the body). Its `## Interfaces` bind like `(user)` decisions: extend them, never contradict one without asking the user (`Entry:`-style quote of the signature in the question). Slice 1 builds the skeleton as specified; a later slice reads the skeleton in the code, where it now lives, and `epic.md` only for orientation. Your `architecture.md` covers this slice: the skeleton components it touches appear as `existing` or `changed`, and a skeleton signature you change with the user's consent is appended to `decisions.md` as a `(user)` line. Local IDs you add start at the slice number times 100 (`R301`, `E301` in slice 3).
