//! Embedded Lua scripting built on [`mlua`].
//!
//! A [`Context`] owns a Lua VM plus a set of keyed [`Script`]s and is
//! assembled with [`ContextBuilder`]. Host functionality is exposed to Lua
//! through [`module`]s (registers, static values, time) registered as
//! global userdata objects.

// Lets the `#[derive(Module)]` macro's emitted `ferrowl_lua::module::Module`
// path resolve when adopted inside this crate.
extern crate self as ferrowl_lua;

mod builder;
mod context;
pub mod module;
mod script;
mod script_state;

pub use builder::ContextBuilder;
pub use context::Context;
pub use mlua::{Error, Result};
pub use script::Script;
pub use script_state::ScriptState;
