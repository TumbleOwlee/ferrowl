# demo — review verdict (stage s1, pass 1)

Verdict: findings — 0 must fix, 1 needs you

Spec — 2 of 2 pinned
Spec changes — MB-R-162 detail, MB-R-161 needs you
Architecture — clean
Standards — clean
TDD — clean
Docs — clean
Not checked — s2 demo TUI run: needs a terminal

Minors (folded into s2)
- standards — ferrowl-modbus client_core.rs `next_delay`: comment restates the code; drop it

Needs user
- MB-R-161 now gives up after 1 hour of retries; keep it?
  - a) revert to retry forever (recommended: S1 decided by the user)
  - b) keep the 1 hour limit
