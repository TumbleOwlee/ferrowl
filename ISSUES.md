# Open Issues

* Removing all predefined values in `EditSelectionDialog` doesn't switch to `EditInputDialog` and messes up `value` input field
* Predefined values is only supported for integer type - should also support all other types
* No option to add predefined values when non exist
* No input field in `EditInputDialog` and `EditSelectionDialog` to display and edit the lua script in `update`
* Background color of whole screen is not set, unused table space is kept default
* Floating box over commandline whenever commandline is in focus to display supported commands
* Support changing register type, e.g. `HoldingRegister` to `Coil`
    * Requires better `EditInputDialog` and `EditSelectionDialog` since coils/discrete inputs only support boolean
* Add compact mode to remove the vertical row margins (currently it is set to 1) to be able to show more registers in the available view area
* Support value and raw value for virtual registers (currently no value is shown, no editing is possible)
* Changing value and type only changes type and drops value because memory is initialized -> Have to set value afterwards
* A client module should not try to read registers with type `WriteOnly`
* Client operations are not updated whenever a definition is changed, thus the client tries to e.g. read wrong or outdated addresses.
* Add command to reload the current module (thus reloading the device configuration) with `:reload`
