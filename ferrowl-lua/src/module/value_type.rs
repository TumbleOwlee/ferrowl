//! A dynamically typed value passed across the Lua/host boundary.

/// A dynamically typed value passed between Lua and the host.
#[derive(Clone, Debug)]
pub enum ValueType {
    Int(i128),
    Float(f64),
    String(String),
    Bool(bool),
    Nil,
}

impl mlua::IntoLua for ValueType {
    fn into_lua(self, lua: &mlua::Lua) -> mlua::Result<mlua::Value> {
        match self {
            ValueType::Int(v) => v.into_lua(lua),
            ValueType::Float(v) => v.into_lua(lua),
            ValueType::String(v) => v.into_lua(lua),
            ValueType::Bool(v) => v.into_lua(lua),
            ValueType::Nil => Ok(mlua::Value::Nil),
        }
    }
}

impl mlua::FromLua for ValueType {
    fn from_lua(value: mlua::Value, _lua: &mlua::Lua) -> mlua::Result<Self> {
        match value {
            mlua::Value::Integer(v) => Ok(ValueType::Int(v as i128)),
            mlua::Value::Number(v) => Ok(ValueType::Float(v)),
            mlua::Value::String(v) => Ok(ValueType::String(v.to_str()?.to_owned())),
            mlua::Value::Boolean(v) => Ok(ValueType::Bool(v)),
            mlua::Value::Nil => Ok(ValueType::Nil),
            other => Err(mlua::Error::FromLuaConversionError {
                from: other.type_name(),
                to: "ValueType".to_string(),
                message: Some("expected number, string or boolean".to_string()),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ValueType;
    use mlua::{FromLua, IntoLua, Lua};

    #[test]
    /// SC-R-009 — the five boundary types (integer, float, string, boolean, nil) each cross from
    /// Lua into a host `ValueType`.
    fn ut_from_lua_accepts_the_five_boundary_types() {
        let lua = Lua::new();
        let cases = [
            ("42", "Int"),
            ("1.5", "Float"),
            ("'hi'", "String"),
            ("true", "Bool"),
            ("nil", "Nil"),
        ];
        for (expr, want) in cases {
            let value = lua.load(expr).eval::<mlua::Value>().unwrap();
            let got = ValueType::from_lua(value, &lua).unwrap();
            let variant = match got {
                ValueType::Int(_) => "Int",
                ValueType::Float(_) => "Float",
                ValueType::String(_) => "String",
                ValueType::Bool(_) => "Bool",
                ValueType::Nil => "Nil",
            };
            assert_eq!(variant, want, "for Lua expression `{expr}`");
        }
    }

    #[test]
    /// SC-R-009 — any other Lua value (table, function) fails conversion with an error rather than
    /// being coerced or silently dropped.
    fn ut_from_lua_rejects_non_scalar_values() {
        let lua = Lua::new();
        for expr in ["{}", "function() end"] {
            let value = lua.load(expr).eval::<mlua::Value>().unwrap();
            assert!(
                ValueType::from_lua(value, &lua).is_err(),
                "`{expr}` must fail conversion, not coerce"
            );
        }
    }

    #[test]
    /// SC-R-009 — each of the five host types converts back into its matching Lua value.
    fn ut_into_lua_round_trips_the_five_types() {
        let lua = Lua::new();
        assert!(ValueType::Int(7).into_lua(&lua).unwrap().is_integer());
        assert!(ValueType::Float(1.5).into_lua(&lua).unwrap().is_number());
        assert!(
            ValueType::String("x".into())
                .into_lua(&lua)
                .unwrap()
                .is_string()
        );
        assert!(ValueType::Bool(true).into_lua(&lua).unwrap().is_boolean());
        assert!(ValueType::Nil.into_lua(&lua).unwrap().is_nil());
    }
}
