# Edit Modbus read ranges in their own module settings dialog

## Goal
Modbus read ranges are edited in a separate module settings dialog (`:module`), so loading a module from a device-config file keeps that file's ranges.
Today the read-range inputs sit in the setup dialog, and confirming it to load a file also applies the dialog's range inputs over the file's ranges.

## Out of scope
- writing the device-config file on confirm — `:write-device` stays the only writer
- read ranges on monitor tabs
- editing any module setting other than read ranges
