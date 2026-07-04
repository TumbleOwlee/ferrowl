//! Per-version action specs driving the per-action send dialog ([`super::action_dialog`]).
//!
//! Each `action_spec(name)` returns the property list + assembler for an action, or `None` to
//! fall back to the raw JSON editor. The rule: an action gets a typed spec unless its payload's
//! REQUIRED field is itself nested (an object) or a list with no optional escape hatch — those
//! stay JSON-only since the flat property-editor can't represent them. Per-version delta files
//! (e.g. `v2_1`) spec only their own version-only actions/overrides and delegate to the prior
//! version's module for everything unchanged.

pub mod v1_6;
pub mod v2_0_1;
pub mod v2_1;
