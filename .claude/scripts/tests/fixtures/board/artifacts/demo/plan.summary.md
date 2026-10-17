# demo — plan summary (rev 1)

Decide: 3 stages, s0 → s1 → s2. Backoff before UI, the tab reads the new state.

Spec since architecture: +E1 ~R1 · fixed IDs 2

| Stage | What | Pins | Files | Stop |
|---|---|---|---|---|
| s0 | Land spec | all 4 entries | 2 (docs/specs/modbus) | — |
| s1 | Retry with backoff in the client task | MB-R-161, MB-E-031 (2) | 1 (ferrowl-modbus client_core) | yes — new public error variant |
| s2 | Render reconnecting in the tab | MB-R-042 (1), S1 | 1 (ferrowl-ui modbus tab) | — |

Verification beyond unit tests:
- s2 — demo TUI
  - stop the demo server, see "reconnecting", restart it, polling resumes
  - cannot be waived without asking

Out of scope, raise separately:
- none

Open: none
