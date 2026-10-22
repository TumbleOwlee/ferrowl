//! Connection status of the host module, as read from Lua.

use mlua::{IntoLua, Lua, Value};

/// A module's displayed connection status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Connected,
    Reconnecting,
    Disconnected,
}

impl ConnectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionStatus::Connected => "CONNECTED",
            ConnectionStatus::Reconnecting => "RECONNECTING",
            ConnectionStatus::Disconnected => "DISCONNECTED",
        }
    }
}

impl IntoLua for ConnectionStatus {
    fn into_lua(self, lua: &Lua) -> mlua::Result<Value> {
        lua.create_string(self.as_str()).map(Value::String)
    }
}

/// Source of the host module's connection status at the moment of the call.
pub trait StatusSource {
    fn connection_status(&self) -> mlua::Result<ConnectionStatus>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_connection_status_into_lua_strings() {
        let lua = Lua::new();
        for (status, want) in [
            (ConnectionStatus::Connected, "CONNECTED"),
            (ConnectionStatus::Reconnecting, "RECONNECTING"),
            (ConnectionStatus::Disconnected, "DISCONNECTED"),
        ] {
            let Value::String(s) = status.into_lua(&lua).unwrap() else {
                panic!("not a string");
            };
            assert_eq!(s.to_str().unwrap(), want);
        }
    }
}
