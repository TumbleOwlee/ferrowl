//! Shared Lua script definition, used by both the OCPP and Modbus device configs and
//! managed by the [`ScriptDialog`](crate::dialog::scripts::ScriptDialog).

use serde::{Deserialize, Serialize};

/// One named Lua simulation script attached to a device type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptDef {
    pub name: String,
    #[serde(default)]
    pub code: String,
    /// Whether the script runs in the simulation loop. Defaults to On (a freshly-created script
    /// and a flag-less file entry are both active).
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}
