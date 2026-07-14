//! The `?` overlay content for script dialogs: the custom Lua bindings available in that dialog's
//! script context. The overlay widget itself lives in [`crate::dialog::help`].

use crate::dialog::help::{BindingSection, HelpOverlay};

/// Which script the overlay is describing bindings for, so it can show only the modules actually
/// reachable from that script's global table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptContext {
    Modbus,
    OcppClient,
    OcppServer,
    Session,
}

// NOTE: keep these tables in sync with the `add_method` registrations in the `ferrowl-lua` module
// crate (`ferrowl-lua/src/module/*.rs`).

static REGISTER_SECTION: BindingSection = BindingSection {
    title: "C_Register",
    entries: &[
        ("C_Register:Get(name)", "read a register's value"),
        ("C_Register:Set(name, value)", "write a register's value"),
    ],
};

static OCPP_CLIENT_SECTION: BindingSection = BindingSection {
    title: "C_OCPP",
    entries: &[
        ("C_OCPP:Get(name)", "read charging-station-level state"),
        (
            "C_OCPP:Set(name, value)",
            "write charging-station-level state",
        ),
        (
            "C_OCPP:<Action>(overrides?)",
            "one method per OCPP action, e.g. StartTransaction({ idTag = \"ABC\" })",
        ),
        (
            "C_OCPP:Connector(id)",
            "accessor scoped to one connector, with its own Get/Set/<Action>",
        ),
    ],
};

static OCPP_SERVER_SECTION: BindingSection = BindingSection {
    title: "C_OCPP",
    entries: &[
        (
            "C_OCPP:GetChargingStations()",
            "list known charging station ids",
        ),
        (
            "C_OCPP:GetConnectors(cs)",
            "list connector ids for a station",
        ),
        (
            "C_OCPP:ChargingStation(cs)",
            "accessor scoped to one station, with its own Get/Set/<Action>",
        ),
        (
            "C_OCPP:Connector(cs, id)",
            "accessor scoped to one connector, with its own Get/Set/<Action>",
        ),
        (
            "<accessor>:<Action>(overrides?)",
            "one method per OCPP action, e.g. MeterValues({ energy = 100 })",
        ),
    ],
};

static MODULE_SECTION: BindingSection = BindingSection {
    title: "C_Module",
    entries: &[
        (
            "C_Module:List()",
            "sorted names of every module in the session",
        ),
        (
            "C_Module:Get(name)",
            "resolve a module by name to a handle (raises if unknown)",
        ),
        (
            "<module>:Type()",
            "module kind, e.g. \"modbus\" or \"ocpp\"",
        ),
        (
            "<module>:Role()",
            "module role, e.g. \"client\" or \"server\"",
        ),
        (
            "<module>:Register()",
            "C_Register-shaped accessor (modbus modules only)",
        ),
        ("<register>:Get(name)", "read a register's value"),
        ("<register>:Set(name, value)", "write a register's value"),
        (
            "<module>:OCPP()",
            "C_OCPP-shaped accessor (ocpp modules only)",
        ),
        (
            "<ocpp>:GetChargingStations()",
            "list known charging station ids",
        ),
        (
            "<ocpp>:GetConnectors(cs)",
            "list connector ids for a station",
        ),
        (
            "<ocpp>:ChargingStation(cs)",
            "accessor scoped to one station, with its own Get/Set/<Action>",
        ),
        (
            "<ocpp>:Connector(cs, id)",
            "accessor scoped to one connector, with its own Get/Set/<Action>",
        ),
        (
            "<ocpp-accessor>:<Action>(json?)",
            "one method per OCPP action, e.g. MeterValues({ energy = 100 })",
        ),
    ],
};

static TIME_SECTION: BindingSection = BindingSection {
    title: "C_Time",
    entries: &[
        ("C_Time:Get()", "seconds elapsed since module start"),
        ("C_Time:GetMs()", "milliseconds elapsed since module start"),
    ],
};

static TEST_SECTION: BindingSection = BindingSection {
    title: "C_Test",
    entries: &[
        (
            "C_Test:Assert(cond, msg)",
            "raise if cond is falsy (nil/false)",
        ),
        ("C_Test:Fail(msg)", "always raise"),
    ],
};

static LOG_SECTION: BindingSection = BindingSection {
    title: "C_Log",
    entries: &[
        (
            "C_Log:Info(message)",
            "append an info line to the module log",
        ),
        (
            "C_Log:Warn(message)",
            "append a warning line to the module log",
        ),
        (
            "C_Log:Error(message)",
            "append an error line to the module log",
        ),
    ],
};

static PRINT_SECTION: BindingSection = BindingSection {
    title: "print",
    entries: &[(
        "print(...)",
        "redirected to the module log, like C_Log:Info",
    )],
};

// Context-specific section(s) first, then the shared `C_Time`/`C_Test`/`C_Log`/`print` sections
// available in every script context.
static MODBUS_SECTIONS: &[&BindingSection] = &[
    &REGISTER_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];
static OCPP_CLIENT_SECTIONS: &[&BindingSection] = &[
    &OCPP_CLIENT_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];
static OCPP_SERVER_SECTIONS: &[&BindingSection] = &[
    &OCPP_SERVER_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];
static SESSION_SECTIONS: &[&BindingSection] = &[
    &MODULE_SECTION,
    &TIME_SECTION,
    &TEST_SECTION,
    &LOG_SECTION,
    &PRINT_SECTION,
];

/// Context-specific section(s) first, then shared `C_Time`, `C_Test`, `C_Log`, `print`.
pub fn sections(ctx: ScriptContext) -> &'static [&'static BindingSection] {
    match ctx {
        ScriptContext::Modbus => MODBUS_SECTIONS,
        ScriptContext::OcppClient => OCPP_CLIENT_SECTIONS,
        ScriptContext::OcppServer => OCPP_SERVER_SECTIONS,
        ScriptContext::Session => SESSION_SECTIONS,
    }
}

/// The Lua-bindings help page for `ctx`.
pub fn lua_help_overlay(ctx: ScriptContext) -> HelpOverlay {
    HelpOverlay::new("Lua Bindings", sections(ctx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ut_modbus_has_register_no_ocpp() {
        let secs = sections(ScriptContext::Modbus);
        assert!(secs.iter().any(|s| s.title == "C_Register"));
        assert!(!secs.iter().any(|s| s.title == "C_OCPP"));
    }

    #[test]
    fn ut_ocpp_server_mentions_get_charging_stations() {
        let secs = sections(ScriptContext::OcppServer);
        let ocpp = secs.iter().find(|s| s.title == "C_OCPP").unwrap();
        assert!(
            ocpp.entries
                .iter()
                .any(|(sig, _)| sig.contains("GetChargingStations"))
        );
    }

    #[test]
    fn ut_ocpp_client_mentions_connector() {
        let secs = sections(ScriptContext::OcppClient);
        let ocpp = secs.iter().find(|s| s.title == "C_OCPP").unwrap();
        assert!(
            ocpp.entries
                .iter()
                .any(|(sig, _)| sig.contains("Connector"))
        );
    }

    #[test]
    fn ut_session_has_module_section() {
        let secs = sections(ScriptContext::Session);
        assert!(secs.iter().any(|s| s.title == "C_Module"));
    }

    #[test]
    fn ut_every_context_includes_shared_sections() {
        for ctx in [
            ScriptContext::Modbus,
            ScriptContext::OcppClient,
            ScriptContext::OcppServer,
            ScriptContext::Session,
        ] {
            let secs = sections(ctx);
            for shared in ["C_Time", "C_Test", "C_Log"] {
                assert!(
                    secs.iter().any(|s| s.title == shared),
                    "{ctx:?} missing {shared}"
                );
            }
        }
    }
}
