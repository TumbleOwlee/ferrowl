# demo — spec diff (rev 5)

## Summary
Goal: A client tab reconnects on its own after the server drops the connection.
Area: modbus — requirements.md `## Client connection`
Revisions:
- rev 2 — added R4 for the error variant (spec-architect)
- rev 3 — fixed IDs (spec-planner)
- rev 4 (2026-10-05) — MB-R-161 kept: retry forever; MB-E-031 kept: drop in-flight requests (user)
- rev 5 (2026-10-06) — synced from the branch: MB-R-161 (spec-author)

## S1 — Server restarts during polling
Given a client tab polling a TCP server
When the server closes the connection
Then the tab shows "reconnecting" and polling resumes once the server is back

## MB-R-161 — new · requirements `## Client connection` · was R1
A TCP client whose connection drops without a user disconnect enters the reconnecting state.

## MB-R-042 — changed · requirements `## Client connection`
Change: dropped connection no longer ends in disconnected
A TCP client whose connection drops **enters the reconnecting state**.

## MB-E-031 — new · edge-cases `## Client connection` · was E1
A request in flight when the connection drops is lost, not replayed.

## MB-E-007 — dropped
- stated "no automatic reconnect"; MB-R-161 replaces it

## Other spec changes
- modbus/api-contract.md › Client config keys › `reconnect` row, default `true` (MB-R-042)

## Deferred
- log line wording per retry — implementation
- an orphan point without owner
