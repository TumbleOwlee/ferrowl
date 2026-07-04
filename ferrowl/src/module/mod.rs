//! Module system: trait definitions, type registry, and per-type implementations.
//!
//! Submodules carry the trait surface ([`view`], [`type_descriptor`]) and the
//! per-type implementations ([`modbus`], [`ocpp`]).

pub mod modbus;
pub mod ocpp;
pub mod type_descriptor;
pub mod type_select;
pub mod view;

use type_descriptor::ModuleTypeDescriptor;

/// All available module types. The UI uses this registry to populate the type-selector
/// in the new-module dialog.
pub static MODULE_TYPES: &[ModuleTypeDescriptor] = &[
    ModuleTypeDescriptor {
        label: "Modbus",
        new_setup_view: || Box::new(modbus::setup::ModbusSetupView::new_create()),
    },
    ModuleTypeDescriptor {
        label: "OCPP",
        new_setup_view: || Box::new(ocpp::setup::OcppSetupView::new()),
    },
];
