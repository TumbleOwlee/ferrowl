# mrcpr — plan (rev 1)

## Shared

Decisions:
- The dialog lives in its own file beside the setup dialog (chosen) [design] — the setup dialog stays small
- Confirm is queued as a pending action and applied in refresh; the dialog prefills from the in-memory device config (chosen) [design] — one apply point
- One function writes both the device-config and the running module's read ranges (chosen) [design] — the two never diverge
- The README command table gains a `:module` row (chosen) [design] — users find the command there
- Esc through the close-confirmation popup (user, rev 4) [spec MB-R-294] — like every other dialog
- trimmed text (chosen, rev 5) [spec MB-R-293] — the old setup-dialog inputs behaved this way
- one stage for the dialog (chosen) [process] — it is reviewed as a unit
