# Open Issues

1. Changing the register type (e.g. `HoldingRegister` to `Coil`) doesn't update the type in the memory layer
    * Memory has to provide an option to change the type
2. Changing to Coil/Discrete input should hide type and type specific fields since coil/discrete input is always boolean
    * Instead of current `Type` selection, show `Type` box with fixed `Boolean` value
