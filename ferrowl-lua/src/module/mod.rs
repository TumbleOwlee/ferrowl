//! Host modules exposed to Lua scripts as global userdata objects.

mod ocpp;
mod register;
mod statics;
mod time;
mod value_type;

pub use ocpp::Ocpp as OcppModule;
pub use ocpp::traits::OcppActions;
pub use register::Register as RegisterModule;
pub use register::traits::{Read, Write};
pub use statics::Statics as StaticsModule;
pub use time::Time as TimeModule;
pub use value_type::ValueType;

/// A host module that can be registered in a Lua context.
pub trait Module {
    /// The Lua global name the module is registered under (e.g. `C_Time`).
    fn module() -> &'static str;
}
