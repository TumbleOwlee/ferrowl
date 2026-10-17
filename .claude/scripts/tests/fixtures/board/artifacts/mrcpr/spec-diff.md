# mrc — spec diff (rev 6)

## Summary
Goal: Modbus read ranges are edited in a separate module settings dialog (`:module`), so loading a module from a device-config file keeps that file's ranges.
Area: modbus — requirements.md `## Module settings dialog`
Revisions:
- rev 5 (2026-10-16) — synced from the branch: MB-R-293, MB-R-294, MB-R-301 (spec-author)
- rev 6 (2026-10-16) — MB-R-294: `Esc` opens the close-confirmation popup instead of closing directly, following UI-R-023 (user)
- rev 6 (2026-10-16) — MB-R-293: the inputs' text is trimmed, a whitespace-only input leaves its field unset, as the old setup-dialog inputs did (review)
- rev 6 (2026-10-16) — MB-R-301: the error box tracks the inputs live instead of appearing only after a refused `Enter` (user)

## S1 — Change read ranges on a running client
Given a Modbus client tab polling with the read ranges of its device config
When the user runs `:module`, changes the `holding` range and presses `Enter`
Then the dialog closes and the next poll cycle reads the new ranges without a reconnect
And no file is written until the user runs `:write-device`

## S2 — A malformed range is refused
Given the module settings dialog is open
When the `coils` input holds the reversed range `10-5` and the user presses `Enter`
Then the `coils` input shows the error border, the error box names `coils` and `10-5`, and the dialog stays open

## S3 — Leaving the dialog without saving
Given the module settings dialog is open with edited inputs
When the user presses `Esc`
Then the dialog closes and the tab's read ranges are unchanged

## S4 — Loading a module keeps the device file's ranges
Given the setup dialog names a device-config file with its own read ranges
When the user confirms the setup dialog
Then the new tab polls that file's read ranges, and the setup dialog showed no range inputs

## S5 — Editing a module keeps its ranges
Given a client tab whose read ranges were changed through `:module`
When the user confirms `:edit`'s setup dialog
Then the tab's read ranges are unchanged

## S6 — No module settings on a monitor tab
Given a Modbus monitor tab
When the user runs `:module`
Then the unknown-command warning is logged and no dialog opens

## MB-R-288 — new · requirements `## Module settings dialog` · was R1
`:module` on a Modbus client or server tab opens the module settings dialog, holding one single-line text input per `read_ranges` field: `holding`, `input`, `coils`, `discrete`.

## MB-R-289 — new · requirements `## Module settings dialog` · was R2
Each input of the module settings dialog (MB-R-288) opens prefilled with the tab's current `read_ranges` value for its field, empty where that field is unset.

## MB-R-290 — new · requirements `## Module settings dialog` · was R3
A Modbus monitor tab does not handle `:module`, so the application logs the unknown-command warning of UI-R-348.

## MB-R-291 — new · requirements `## Module settings dialog` · was R4
A module settings dialog input (MB-R-288) whose text holds a malformed or reversed range entry paints the error border (UI-R-068) while that text stands.

## MB-R-292 — new · requirements `## Module settings dialog` · was R5
`Enter` in the module settings dialog is refused while any input fails MB-R-291's check: the dialog stays open and the tab's `read_ranges` are unchanged.

## MB-R-293 — new · requirements `## Module settings dialog` · was R6
Confirming a module settings dialog whose inputs all parse sets the tab's in-memory device-config `read_ranges` to the inputs' trimmed text, a blank input leaving its field unset.

## MB-R-294 — new · requirements `## Module settings dialog` · was R7
Closing the module settings dialog through its close-confirmation popup (UI-R-023) leaves the tab's `read_ranges` unchanged, whether or not an input fails MB-R-291's check.

## MB-R-295 — new · requirements `## Module settings dialog` · was R8
A running client polls with the `read_ranges` confirmed through the module settings dialog (MB-R-293, MB-R-083) from its next poll cycle on, without reconnecting.

## MB-R-296 — new · requirements `## Module settings dialog` · was R9
Confirming the module settings dialog (MB-R-293) declares the gap cells of the confirmed `read_ranges` (MB-R-084) in the tab's store, a rejected declaration logged per MB-R-129.

## MB-R-297 — new · requirements `## Module settings dialog` · was R10
Confirming the module settings dialog (MB-R-293) writes no file; `:write-device` saves the confirmed `read_ranges` into the device config.

## MB-R-298 — new · requirements `## Module settings dialog` · was R11
The Modbus setup dialog, opened by the new-module flow, `:load`, `:edit` or `:clone`, carries no read-range input.

## MB-R-299 — new · requirements `## Module settings dialog` · was R12
A tab created by confirming a Modbus setup dialog that names a device-config path takes that file's `read_ranges`.

## MB-R-300 — new · requirements `## Module settings dialog` · was R13
Confirming `:edit`'s Modbus setup dialog leaves the tab's `read_ranges` unchanged.

## MB-R-301 — new · requirements `## Module settings dialog` · was R14
The module settings dialog's error message box tracks its inputs live: while any input fails MB-R-291's check it names the first failing input's field and its malformed entry, and it disappears once every input passes, as the add/edit register dialog shows its inline error (MB-R-224).

## MB-E-105 — new · edge-cases `## Module settings dialog` · was E1
Gap cells declared for a read range that a later module settings dialog confirm removes (MB-R-296) stay declared in the store, so a read spanning them still succeeds.

## Other spec changes
- tui/api-contract.md › Modbus module, row after `:script` › ``| `:module` | — | Open the module settings dialog (read ranges) | UI-R-018, MB-R-288 |``
- tui/api-contract.md › note under the monitor table › `` `:set` (nothing to write), `:script` (no Lua surface) and `:module` (no `read_ranges`, MB-R-290) are omitted for this role — unrecognized commands on a monitor view, not errors. ``
- modbus/api-contract.md › Device config, `read_ranges` row, `Req` column › `MB-R-082, MB-R-083, MB-R-288, MB-R-293`
