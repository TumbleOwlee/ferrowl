//! Module system: trait definitions, type registry, and Modbus implementation.
//!
//! Submodules carry the trait surface ([`view`], [`type_descriptor`]) and the
//! Modbus-specific implementation ([`modbus`]).

pub mod modbus;
pub mod type_descriptor;
pub mod view;

pub use modbus::{FileSink, Module, ModuleLog, ModuleMemory, VirtualStore};
pub(crate) use modbus::{append, default_value, str_to_value};

use type_descriptor::ModuleTypeDescriptor;

/// All available module types. The UI uses this registry to populate the type-selector
/// in the new-module dialog.
pub static MODULE_TYPES: &[ModuleTypeDescriptor] = &[ModuleTypeDescriptor {
    label: "Modbus",
    new_setup_view: || Box::new(modbus::setup::ModbusSetupView::new_create()),
}];
