# mrcpr — decisions

- The dialog lives in its own file beside the setup dialog (chosen) [design] — the setup dialog stays small · planner
- Confirm is queued as a pending action and applied in refresh; the dialog prefills from the in-memory device config (chosen) [design] — one apply point · planner
- One function writes both the device-config and the running module's read ranges (chosen) [design] — the two never diverge · planner
- The README command table gains a `:module` row (chosen) [design] — users find the command there · planner
- Esc through the close-confirmation popup (user) [spec MB-R-294] — like every other dialog · planner
- trimmed text (chosen) [spec MB-R-293] — the old setup-dialog inputs behaved this way · planner
- one stage for the dialog (chosen) [process] — it is reviewed as a unit · planner
