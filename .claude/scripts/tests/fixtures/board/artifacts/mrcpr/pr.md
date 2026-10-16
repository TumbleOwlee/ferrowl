# Edit Modbus read ranges in their own module settings dialog

## Why
Modbus read ranges are edited in a separate module settings dialog (`:module`), so loading a module from a device-config file keeps that file's ranges.
Today confirming the setup dialog to load a file applies the dialog's range inputs over the file's ranges.

## Changes
- ferrowl `module::modbus::settings_dialog` — new module settings dialog: four range inputs prefilled from the device config, error border per input, live error box, `Enter` refused while an input fails, `Esc` through the close-confirmation popup (MB-R-288, MB-R-289, MB-R-291, MB-R-292, MB-R-294, MB-R-301)
- ferrowl `module::modbus::view` — `:module` command on client and server tabs, confirm adopts the ranges in memory and in the running module, writes no file (MB-R-288, MB-R-293, MB-R-296, MB-R-297)
- ferrowl `module::modbus::monitor::view` — `:module` left unhandled on monitor tabs (MB-R-290)
- ferrowl `module::modbus::module` — read ranges replaced at runtime, poll operations rebuilt in place without reconnecting, gap cells of the new ranges declared (MB-R-295, MB-R-296, MB-E-105)
- ferrowl `module::modbus::config::device` — strict range check shared by the dialog (MB-R-291)
- ferrowl `module::modbus::setup_dialog`, `setup` — setup dialog drops its read-range inputs; a device file's ranges and the running module's ranges survive confirm and `:edit` (MB-R-298, MB-R-299, MB-R-300)
- docs/specs/modbus — 15 entries added, 0 changed
- docs/specs/tui — `:module` command row, monitor-table note extended
- README — `:module` row in the command table, screenshot script reference fixed, screenshots regenerated

## Tests
15 of 15 entries pinned
- MB-R-295 — pinned by the module test of the rebuilt poll operations, plus the demo run's no-reconnect check

## Verification beyond the gauntlet
- user ran the demo TUI before the final review, on the tree before the live-error-box rework
  - `:module` opened on the client and server tabs, prefilled from the device's `read_ranges`
  - an invalid input showed the error border, and `Enter` was refused
  - `Esc`, then confirming the close popup, left the ranges unchanged
  - confirming new ranges on a running client kept polling without a reconnect (MB-R-295)
  - `:wd` saved the confirmed ranges
  - the error box refreshed only on confirm, which became the MB-R-301 live-box change
- user regenerated all screenshots with `images/capture.py`; the new-module dialog shows no range inputs
- not run: a demo re-check of the live error box after the rework, which `ut_error_box_tracks_inputs_live` pins
