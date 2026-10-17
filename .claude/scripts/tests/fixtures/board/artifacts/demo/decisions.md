# demo — decisions

- no confirm dialog (user) [scope] · author
- reconnecting is its own state (chosen) [spec MB-R-161] — the tab must tell it from disconnected · author
- drop it (chosen) [spec MB-E-031] — a replayed write may apply twice · architect
- policy owned by the client task (chosen) [design] — RTU and TCP share it · architect
- reconnecting is its own state (chosen) [spec MB-R-042] — the tab must tell it from disconnected · architect
- one stage per crate (chosen) [process] — the UI change is reviewed apart · planner
- backoff state lives in the client task (chosen) [design] — RTU and TCP share it · planner
- retry forever, no cap (chosen) [spec MB-R-161] — the user never wants a dead tab · planner
- drop the in-flight request (user) [spec MB-E-031] — a replayed write may apply twice · planner
- legacy line without a tag (chosen) — skipped by every renderer · planner
