# mrcpr — architecture (rev 1)

## Components

| Component | Kind | Crate › module | Change | Role |
|---|---|---|---|---|
| `ModuleSettingsDialog` | struct | ferrowl › module::modbus::settings_dialog | new | four `RangeSpec` inputs (holding, input, coils, discrete), live error box, close confirmation; `resolve` → `Result<ReadRanges, String>`, `confirm` → `Option<ReadRanges>` |
| `RangeSpec` | struct | ferrowl › module::modbus::settings_dialog | new | `Validate` impl over `check_range_spec` for one range input |
| `ModbusModuleView::apply_read_ranges` | fn | ferrowl › module::modbus::view::mutate | new | async, `&mut self`; writes one `ReadRanges` to the device config and the running module |
| `ModbusViewOverlay` | enum | ferrowl › module::modbus::view | changed | new `Settings(Box<ModuleSettingsDialog>)` variant |
| `PendingAction` | enum | ferrowl › module::modbus::view::overlay | changed | new `SetReadRanges(ReadRanges)` variant, applied in refresh |
| `ModbusCmd` | enum | ferrowl › module::modbus::view | changed | new `Module` variant, 13 commands |
