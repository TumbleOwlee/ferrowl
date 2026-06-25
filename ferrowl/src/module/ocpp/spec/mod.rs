//! Per-version action specs driving the per-action send dialog ([`super::action_dialog`]).
//!
//! Each `action_spec(name)` returns the property list + assembler for an action, or `None` for
//! actions still handled by the raw JSON editor (complex/nested — Stage 2). Stage 1 covers the
//! flat actions.

pub mod v1_6;
pub mod v2_0_1;
