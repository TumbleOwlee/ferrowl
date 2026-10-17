# demo — plan (rev 1)

## Shared

Decisions:
- one stage per crate (chosen) [process] — the UI change is reviewed apart
- backoff state lives in the client task (chosen) [design] — RTU and TCP share it
- retry forever, no cap (chosen, rev 4) [spec MB-R-161] — the user never wants a dead tab
- drop the in-flight request (user, rev 2) [spec MB-E-031] — a replayed write may apply twice
- legacy line without a tag (chosen) — skipped by every renderer
