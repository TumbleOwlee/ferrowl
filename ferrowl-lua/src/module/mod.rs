//! Host modules exposed to Lua scripts as global userdata objects.

mod register;
mod statics;
mod time;

pub use register::Register as RegisterModule;
pub use register::traits::{Read, Write};
pub use statics::Statics as StaticsModule;
pub use time::Time as TimeModule;

/// A dynamically typed value passed between Lua and the host.
pub enum ValueType {
    Int(i128),
    Float(f64),
    String(String),
    Bool(bool),
}

/// A host module that can be registered in a Lua context.
pub trait Module {
    /// The Lua global name the module is registered under (e.g. `C_Time`).
    fn module() -> &'static str;
}
