//! Shared Lua script definition, used by both the OCPP and Modbus device configs and
//! managed by the [`ScriptDialog`](crate::dialog::scripts::ScriptDialog).

use serde::{Deserialize, Serialize};

/// One named Lua simulation script attached to a device type. `T` is the trigger kind the
/// owning module accepts: [`ModbusTrigger`], [`OcppTrigger`] or [`SessionTrigger`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptDef<T> {
    pub name: String,
    #[serde(default)]
    pub code: String,
    /// Whether the script runs in the simulation loop. Defaults to On (a freshly-created script
    /// and a flag-less file entry are both active).
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// What starts the script instead of the sim loop; `None` runs it every cycle.
    #[serde(default = "no_trigger", skip_serializing_if = "Option::is_none")]
    pub trigger: Option<T>,
}

fn default_true() -> bool {
    true
}

fn no_trigger<T>() -> Option<T> {
    None
}

/// Trigger of a Modbus device script: a fixed period or a register event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModbusTrigger {
    Interval {
        #[serde(deserialize_with = "seconds")]
        seconds: f64,
    },
    Register {
        registers: Selection,
        #[serde(default)]
        every_write: bool,
    },
}

/// Trigger of an OCPP device script: a fixed period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum OcppTrigger {
    Interval {
        #[serde(deserialize_with = "seconds")]
        seconds: f64,
    },
}

/// Trigger of a session script: a fixed period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionTrigger {
    Interval {
        #[serde(deserialize_with = "seconds")]
        seconds: f64,
    },
}

/// Key under which `serde_json`'s `arbitrary_precision` mode carries a number's digits.
const NUMBER_TOKEN: &str = "$serde_json::private::Number";

/// Reads an interval's `seconds` from an integer or a float.
///
/// A direct visitor, because an internally tagged enum buffers its fields and `serde_json`'s
/// `arbitrary_precision` mode (enabled transitively by `rust-ocpp`'s dependencies) turns a
/// buffered JSON number into a single-entry map that `f64`'s own deserializer rejects.
fn seconds<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, MapAccess, Visitor};

    struct SecondsVisitor;

    impl<'de> Visitor<'de> for SecondsVisitor {
        type Value = f64;

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a number of seconds")
        }

        fn visit_i64<E>(self, v: i64) -> Result<f64, E> {
            Ok(v as f64)
        }

        fn visit_u64<E>(self, v: u64) -> Result<f64, E> {
            Ok(v as f64)
        }

        fn visit_f64<E>(self, v: f64) -> Result<f64, E> {
            Ok(v)
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<f64, A::Error> {
            let key: Option<String> = map.next_key()?;
            if key.as_deref() != Some(NUMBER_TOKEN) {
                return Err(A::Error::custom("expected a number of seconds"));
            }
            let raw: String = map.next_value()?;
            raw.parse()
                .map_err(|_| A::Error::custom(format!("invalid number: {raw}")))
        }
    }

    deserializer.deserialize_any(SecondsVisitor)
}

/// The registers a register trigger watches: named, or every register.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Selection {
    Names(Vec<String>),
    All(AllMarker),
}

/// The literal `"all"` in a [`Selection`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AllMarker {
    #[serde(rename = "all")]
    All,
}

/// A module's script list, typed by the trigger kind it holds.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptList {
    Modbus(Vec<ScriptDef<ModbusTrigger>>),
    Ocpp(Vec<ScriptDef<OcppTrigger>>),
}

/// A script's trigger does not fit the module it is copied to.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("script '{script}' has a trigger that does not fit this module")]
pub struct TriggerMismatch {
    pub script: String,
}

impl ScriptList {
    pub fn len(&self) -> usize {
        match self {
            Self::Modbus(s) => s.len(),
            Self::Ocpp(s) => s.len(),
        }
    }

    /// The list as Modbus scripts. A script without a trigger converts; a triggered script of
    /// another kind names itself in the error.
    pub fn into_modbus(self) -> Result<Vec<ScriptDef<ModbusTrigger>>, TriggerMismatch> {
        match self {
            Self::Modbus(s) => Ok(s),
            Self::Ocpp(s) => retype(s, |OcppTrigger::Interval { seconds }| {
                Some(ModbusTrigger::Interval { seconds })
            }),
        }
    }

    /// The list as OCPP scripts; see [`ScriptList::into_modbus`].
    pub fn into_ocpp(self) -> Result<Vec<ScriptDef<OcppTrigger>>, TriggerMismatch> {
        match self {
            Self::Ocpp(s) => Ok(s),
            Self::Modbus(s) => retype(s, |t| match t {
                ModbusTrigger::Interval { seconds } => Some(OcppTrigger::Interval { seconds }),
                ModbusTrigger::Register { .. } => None,
            }),
        }
    }
}

/// Re-type scripts through `convert`, which maps a present trigger to its counterpart or
/// `None` when it has none.
fn retype<A, B>(
    scripts: Vec<ScriptDef<A>>,
    convert: impl Fn(A) -> Option<B>,
) -> Result<Vec<ScriptDef<B>>, TriggerMismatch> {
    scripts
        .into_iter()
        .map(|s| {
            let trigger = match s.trigger {
                None => None,
                Some(t) => Some(convert(t).ok_or_else(|| TriggerMismatch {
                    script: s.name.clone(),
                })?),
            };
            Ok(ScriptDef {
                name: s.name,
                code: s.code,
                enabled: s.enabled,
                trigger,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// SC-R-062 — a script's persisted shape follows config-session's envelope: `code` and
    /// `enabled` are optional on read (default empty / on), and a full round-trip through both
    /// TOML and JSON files preserves the value.
    fn ut_script_def_persists_per_config_session_envelope() {
        use crate::convert::{Converter, FileType};
        use ferrowl_test_support::reserve_temp_dir;

        let dir = reserve_temp_dir("ferrowl_script_def_envelope");
        let toml_path = dir.join("minimal.toml");
        std::fs::write(&toml_path, "name = \"s\"\n").unwrap();
        let parsed: ScriptDef<ModbusTrigger> =
            Converter::load(toml_path.to_str().unwrap(), FileType::Toml).unwrap();
        assert_eq!(
            parsed,
            ScriptDef {
                trigger: None,
                name: "s".into(),
                code: String::new(),
                enabled: true,
            }
        );

        let full = ScriptDef {
            trigger: None,
            name: "s".into(),
            code: "C_Log:Info(\"x\")".into(),
            enabled: false,
        };
        for (path, ty) in [
            (dir.join("full.toml"), FileType::Toml),
            (dir.join("full.json"), FileType::Json),
        ] {
            let path = path.to_str().unwrap();
            Converter::save(&full, path, ty).unwrap();
            let back: ScriptDef<ModbusTrigger> = Converter::load(path, ty).unwrap();
            assert_eq!(back, full);
        }
    }

    fn parse<T: serde::de::DeserializeOwned>(
        name: &str,
        text: &str,
        ty: crate::convert::FileType,
    ) -> Result<T, crate::convert::Error> {
        let dir = ferrowl_test_support::reserve_temp_dir("ferrowl_script_trigger");
        let path = dir.join(name);
        std::fs::write(&path, text).unwrap();
        crate::convert::Converter::load(path.to_str().unwrap(), ty)
    }

    fn register(names: &[&str], every_write: bool) -> ModbusTrigger {
        ModbusTrigger::Register {
            registers: Selection::Names(names.iter().map(|n| n.to_string()).collect()),
            every_write,
        }
    }

    fn script<T>(trigger: Option<T>) -> ScriptDef<T> {
        ScriptDef {
            name: "s".into(),
            code: String::new(),
            enabled: true,
            trigger,
        }
    }

    #[test]
    /// SC-R-022, SC-R-097 — a script entry with no trigger field loads with no trigger.
    fn ut_script_def_has_no_trigger_by_default() {
        use crate::convert::FileType;
        let toml: ScriptDef<ModbusTrigger> =
            parse("none.toml", "name = \"s\"\n", FileType::Toml).unwrap();
        assert_eq!(toml.trigger, None);
        let json: ScriptDef<ModbusTrigger> =
            parse("none.json", r#"{"name": "s"}"#, FileType::Json).unwrap();
        assert_eq!(json.trigger, None);
    }

    #[test]
    /// SC-R-086, SC-R-100 — a script's trigger is none, an interval or a register event,
    /// nothing else; the interval's `seconds` parses from a float or an integer.
    fn ut_trigger_is_none_interval_or_register() {
        use crate::convert::FileType;
        let reg: ScriptDef<ModbusTrigger> = parse(
            "reg.toml",
            "name = \"s\"\n[trigger]\nkind = \"register\"\nregisters = [\"a\"]\n",
            FileType::Toml,
        )
        .unwrap();
        assert_eq!(reg.trigger, Some(register(&["a"], false)));
        let every = Some(ModbusTrigger::Interval { seconds: 5.0 });
        for (name, text, ty) in [
            (
                "float.toml",
                "name = \"s\"\n[trigger]\nkind = \"interval\"\nseconds = 5.0\n",
                FileType::Toml,
            ),
            (
                "int.toml",
                "name = \"s\"\n[trigger]\nkind = \"interval\"\nseconds = 5\n",
                FileType::Toml,
            ),
            (
                "float.json",
                r#"{"name":"s","trigger":{"kind":"interval","seconds":5.0}}"#,
                FileType::Json,
            ),
            (
                "int.json",
                r#"{"name":"s","trigger":{"kind":"interval","seconds":5}}"#,
                FileType::Json,
            ),
        ] {
            let def: ScriptDef<ModbusTrigger> = parse(name, text, ty).unwrap();
            assert_eq!(def.trigger, every, "{name}");
        }
        let unknown = parse::<ScriptDef<ModbusTrigger>>(
            "unknown.toml",
            "name = \"s\"\n[trigger]\nkind = \"message\"\n",
            FileType::Toml,
        );
        assert!(unknown.is_err());
        let missing_kind = parse::<ScriptDef<ModbusTrigger>>(
            "nokind.toml",
            "name = \"s\"\n[trigger]\nregisters = [\"a\"]\n",
            FileType::Toml,
        );
        assert!(missing_kind.is_err());
    }

    #[test]
    /// SC-R-086 — `seconds` must be a number: a table or a string is rejected.
    fn ut_interval_seconds_rejects_non_numbers() {
        use crate::convert::FileType;
        for (name, text, ty) in [
            (
                "table.toml",
                "name = \"s\"\n[trigger]\nkind = \"interval\"\nseconds = { a = \"5\" }\n",
                FileType::Toml,
            ),
            (
                "str.toml",
                "name = \"s\"\n[trigger]\nkind = \"interval\"\nseconds = \"5\"\n",
                FileType::Toml,
            ),
            (
                "table.json",
                r#"{"name":"s","trigger":{"kind":"interval","seconds":{"a":"5"}}}"#,
                FileType::Json,
            ),
        ] {
            assert!(
                parse::<ScriptDef<ModbusTrigger>>(name, text, ty).is_err(),
                "{name}"
            );
        }
    }

    #[test]
    /// SC-R-086 — an interval trigger needs `seconds` and takes no other key.
    fn ut_interval_trigger_requires_seconds() {
        use crate::convert::FileType;
        let missing = parse::<ScriptDef<ModbusTrigger>>(
            "missing.toml",
            "name = \"s\"\n[trigger]\nkind = \"interval\"\n",
            FileType::Toml,
        );
        assert!(missing.is_err());
        let extra = parse::<ScriptDef<ModbusTrigger>>(
            "extra.toml",
            "name = \"s\"\n[trigger]\nkind = \"interval\"\nseconds = 5\nevery_write = true\n",
            FileType::Toml,
        );
        assert!(extra.is_err());
    }

    #[test]
    /// SC-R-086 — an interval script copies between a Modbus and an OCPP list, keeping its
    /// period.
    fn ut_interval_script_copies_between_modbus_and_ocpp() {
        let modbus = vec![script(Some(ModbusTrigger::Interval { seconds: 2.5 }))];
        let ocpp = ScriptList::Modbus(modbus.clone()).into_ocpp().unwrap();
        assert_eq!(
            ocpp,
            vec![script(Some(OcppTrigger::Interval { seconds: 2.5 }))]
        );
        assert_eq!(ScriptList::Ocpp(ocpp).into_modbus().unwrap(), modbus);
    }

    #[test]
    /// `registers` is a list of names or the string `all`, in both file types.
    fn ut_registers_selects_names_or_all() {
        use crate::convert::FileType;
        let all_toml: ScriptDef<ModbusTrigger> = parse(
            "all.toml",
            "name = \"s\"\n[trigger]\nkind = \"register\"\nregisters = \"all\"\n",
            FileType::Toml,
        )
        .unwrap();
        let all_json: ScriptDef<ModbusTrigger> = parse(
            "all.json",
            r#"{"name":"s","trigger":{"kind":"register","registers":"all"}}"#,
            FileType::Json,
        )
        .unwrap();
        let all = Some(ModbusTrigger::Register {
            registers: Selection::All(AllMarker::All),
            every_write: false,
        });
        assert_eq!(all_toml.trigger, all);
        assert_eq!(all_json.trigger, all);
        let names: ScriptDef<ModbusTrigger> = parse(
            "names.json",
            r#"{"name":"s","trigger":{"kind":"register","registers":["a","b"],"every_write":true}}"#,
            FileType::Json,
        )
        .unwrap();
        assert_eq!(names.trigger, Some(register(&["a", "b"], true)));
        let other = parse::<ScriptDef<ModbusTrigger>>(
            "other.json",
            r#"{"name":"s","trigger":{"kind":"register","registers":"most"}}"#,
            FileType::Json,
        );
        assert!(other.is_err());
    }

    #[test]
    /// CS-R-077 — a script with no trigger is saved with no trigger field.
    fn ut_no_trigger_is_saved_with_no_trigger_field() {
        use crate::convert::{Converter, FileType};
        let dir = ferrowl_test_support::reserve_temp_dir("ferrowl_script_no_trigger");
        for (file, ty) in [("s.toml", FileType::Toml), ("s.json", FileType::Json)] {
            let path = dir.join(file);
            Converter::save(&script::<ModbusTrigger>(None), path.to_str().unwrap(), ty).unwrap();
            let text = std::fs::read_to_string(&path).unwrap();
            assert!(!text.contains("trigger"), "{text}");
        }
    }

    #[test]
    /// CS-R-080 — a trigger reloads equal from TOML and JSON, for names and `all` and both
    /// `every_write` values.
    fn ut_trigger_reloads_equal() {
        use crate::convert::{Converter, FileType};
        let dir = ferrowl_test_support::reserve_temp_dir("ferrowl_script_trigger_reload");
        let triggers = [
            register(&["a", "b"], false),
            register(&["a"], true),
            ModbusTrigger::Register {
                registers: Selection::All(AllMarker::All),
                every_write: false,
            },
            ModbusTrigger::Register {
                registers: Selection::All(AllMarker::All),
                every_write: true,
            },
        ];
        for (n, trigger) in triggers.into_iter().enumerate() {
            let def = script(Some(trigger));
            for (file, ty) in [
                (format!("t{n}.toml"), FileType::Toml),
                (format!("t{n}.json"), FileType::Json),
            ] {
                let path = dir.join(file);
                let path = path.to_str().unwrap();
                Converter::save(&def, path, ty).unwrap();
                let back: ScriptDef<ModbusTrigger> = Converter::load(path, ty).unwrap();
                assert_eq!(back, def);
            }
        }
    }

    #[test]
    /// UI-E-174 — converting a Modbus list to OCPP names the first script whose trigger does
    /// not fit.
    fn ut_into_ocpp_names_the_first_triggered_script() {
        let mut plain = script::<ModbusTrigger>(None);
        plain.name = "plain".into();
        let mut first = script(Some(register(&["a"], false)));
        first.name = "first".into();
        let mut second = script(Some(register(&["b"], false)));
        second.name = "second".into();
        let err = ScriptList::Modbus(vec![plain, first, second])
            .into_ocpp()
            .unwrap_err();
        assert_eq!(
            err,
            TriggerMismatch {
                script: "first".into()
            }
        );
        assert!(err.to_string().contains("'first'"), "{err}");
    }

    #[test]
    /// UI-R-385 — a list with no triggers converts across module kinds, a same-kind list
    /// passes whole.
    fn ut_cross_kind_list_with_no_triggers_converts() {
        let modbus = vec![script::<ModbusTrigger>(None)];
        let ocpp = ScriptList::Modbus(modbus.clone()).into_ocpp().unwrap();
        assert_eq!(ocpp, vec![script::<OcppTrigger>(None)]);
        assert_eq!(ScriptList::Ocpp(ocpp).into_modbus().unwrap(), modbus);
        let triggered = vec![script(Some(register(&["a"], true)))];
        assert_eq!(
            ScriptList::Modbus(triggered.clone()).into_modbus().unwrap(),
            triggered
        );
    }
}
