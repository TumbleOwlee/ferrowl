//! The `?` overlay content for script dialogs: the custom Lua bindings available in that dialog's
//! script context, rendered from the typed method tables in `ferrowl_lua::module::docs`. The
//! overlay widget itself lives in [`crate::dialog::help`].

use crate::dialog::help::{HelpOverlay, HelpSection};
use ferrowl_lua::module::docs::{self, MethodDoc};

/// Which script the overlay is describing bindings for, so it can show only the modules actually
/// reachable from that script's global table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptContext {
    Modbus,
    OcppClient,
    OcppServer,
    Session,
}

/// A script context plus the per-version OCPP action names its help page lists.
pub enum LuaBindings {
    Modbus,
    OcppClient {
        cs_actions: Vec<&'static str>,
        conn_actions: Vec<&'static str>,
    },
    OcppServer {
        station_actions: Vec<&'static str>,
        conn_actions: Vec<&'static str>,
    },
    Session,
}

impl LuaBindings {
    pub fn context(&self) -> ScriptContext {
        match self {
            LuaBindings::Modbus => ScriptContext::Modbus,
            LuaBindings::OcppClient { .. } => ScriptContext::OcppClient,
            LuaBindings::OcppServer { .. } => ScriptContext::OcppServer,
            LuaBindings::Session => ScriptContext::Session,
        }
    }
}

fn entries(receiver: &str, tables: &[&[MethodDoc]]) -> Vec<(String, String)> {
    tables
        .iter()
        .flat_map(|t| t.iter())
        .map(|m| (m.signature(receiver), m.summary.to_string()))
        .collect()
}

fn action_entries(receiver: &str, actions: &[&'static str]) -> Vec<(String, String)> {
    actions
        .iter()
        .map(|a| {
            (
                docs::OCPP_ACTION.action_signature(receiver, a),
                docs::OCPP_ACTION.summary.to_string(),
            )
        })
        .collect()
}

fn section(title: &str, entries: Vec<(String, String)>) -> HelpSection {
    HelpSection {
        title: title.to_string(),
        entries,
    }
}

const GENERIC_ACTION_NOTE: &str = "one method per OCPP action; the set depends on the target module's OCPP version, and a client module's <ocpp> takes the same calls";

/// Context-specific section(s) first, then shared `C_Time`, `C_Test`, `C_Log`, `print`.
pub fn sections(bindings: &LuaBindings) -> Vec<HelpSection> {
    let mut out = match bindings {
        LuaBindings::Modbus => vec![section(
            "C_Register",
            entries("C_Register", &[docs::REGISTER]),
        )],
        LuaBindings::OcppClient {
            cs_actions,
            conn_actions,
        } => {
            let mut cs = entries("C_OCPP", &[docs::OCPP_STATE, docs::OCPP_CLIENT]);
            cs.extend(action_entries("C_OCPP", cs_actions));
            let mut conn = entries("<conn>", &[docs::OCPP_STATE]);
            conn.extend(action_entries("<conn>", conn_actions));
            vec![section("C_OCPP", cs), section("C_OCPP:Connector(id)", conn)]
        }
        LuaBindings::OcppServer {
            station_actions,
            conn_actions,
        } => {
            let mut station = entries("<cs>", &[docs::OCPP_STATE]);
            station.extend(action_entries("<cs>", station_actions));
            let mut conn = entries("<conn>", &[docs::OCPP_STATE]);
            conn.extend(action_entries("<conn>", conn_actions));
            vec![
                section("C_OCPP", entries("C_OCPP", &[docs::OCPP_SERVER])),
                section("C_OCPP:ChargingStation(cs)", station),
                section("C_OCPP:Connector(cs, id)", conn),
            ]
        }
        LuaBindings::Session => {
            let mut scope = entries("<scope>", &[docs::OCPP_STATE]);
            scope.push((
                docs::OCPP_ACTION.signature("<scope>"),
                GENERIC_ACTION_NOTE.to_string(),
            ));
            vec![
                section("C_Module", entries("C_Module", &[docs::MODULE_DIR])),
                section("ModuleHandle", entries("<module>", &[docs::MODULE_HANDLE])),
                section(
                    "Register() accessor",
                    entries("<register>", &[docs::REGISTER]),
                ),
                section(
                    "OCPP() accessor, client module",
                    entries("<ocpp>", &[docs::OCPP_STATE, docs::OCPP_CLIENT]),
                ),
                section(
                    "OCPP() accessor, server module",
                    entries("<ocpp>", &[docs::OCPP_SERVER]),
                ),
                section("OCPP scope accessor", scope),
            ]
        }
    };
    out.push(section("C_Time", entries("C_Time", &[docs::TIME])));
    out.push(section("C_Test", entries("C_Test", &[docs::TEST])));
    out.push(section("C_Log", entries("C_Log", &[docs::LOG])));
    out.push(section(
        "print",
        vec![(docs::PRINT.signature(""), docs::PRINT.summary.to_string())],
    ));
    out
}

/// The Lua-bindings help page for `bindings`.
pub fn lua_help_overlay(bindings: &LuaBindings) -> HelpOverlay {
    HelpOverlay::new("Lua Bindings", sections(bindings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::ocpp::client::lua_sim::ClientFields;
    use crate::module::ocpp::client::lua_sim::OcppFields;
    use crate::module::ocpp::client::v1_6::state::CsState as Cs16;
    use crate::module::ocpp::client::v2_0_1::state::CsState as Cs201;
    use crate::module::ocpp::server::view::ServerVersion;
    use ferrowl_ocpp::{V1_6, V2_0_1};

    fn client(cs: Vec<&'static str>, conn: Vec<&'static str>) -> LuaBindings {
        LuaBindings::OcppClient {
            cs_actions: cs,
            conn_actions: conn,
        }
    }

    fn client16() -> LuaBindings {
        client(Cs16::cs_actions(), Cs16::conn_actions())
    }

    fn client201() -> LuaBindings {
        client(Cs201::cs_actions(), Cs201::conn_actions())
    }

    fn server_actions<V: ServerVersion>() -> (Vec<&'static str>, Vec<&'static str>) {
        (
            <V::Cs as OcppFields>::actions(),
            <V::Conn as OcppFields>::actions(),
        )
    }

    fn server<V: ServerVersion>() -> LuaBindings {
        let (station_actions, conn_actions) = server_actions::<V>();
        LuaBindings::OcppServer {
            station_actions,
            conn_actions,
        }
    }

    fn server16() -> LuaBindings {
        server::<V1_6>()
    }

    fn server201() -> LuaBindings {
        server::<V2_0_1>()
    }

    fn every_context() -> Vec<LuaBindings> {
        vec![
            LuaBindings::Modbus,
            client16(),
            client201(),
            server16(),
            server201(),
            LuaBindings::Session,
        ]
    }

    fn section<'a>(secs: &'a [HelpSection], title: &str) -> &'a HelpSection {
        secs.iter()
            .find(|s| s.title == title)
            .unwrap_or_else(|| panic!("no section '{title}'"))
    }

    fn assert_lists(sec: &HelpSection, docs: &[MethodDoc]) {
        for m in docs {
            let needle = format!(":{}(", m.name);
            assert!(
                sec.entries.iter().any(|(sig, _)| sig.contains(&needle)),
                "section '{}' misses {}",
                sec.title,
                m.name
            );
        }
    }

    fn assert_names(sec: &HelpSection, names: &[&str]) {
        for n in names {
            let needle = format!(":{n}(");
            assert!(
                sec.entries.iter().any(|(sig, _)| sig.contains(&needle)),
                "section '{}' misses action {n}",
                sec.title
            );
        }
    }

    #[test]
    /// UI-R-378 — `C_Register` lists every documented method, `Has` and `ConnectionStatus` included.
    fn ut_modbus_lists_every_register_method() {
        let secs = sections(&LuaBindings::Modbus);
        let reg = section(&secs, "C_Register");
        assert_lists(reg, docs::REGISTER);
        for name in ["Has", "ConnectionStatus"] {
            assert!(reg.entries.iter().any(|(s, _)| s.contains(name)));
        }
        assert!(!secs.iter().any(|s| s.title == "C_OCPP"));
    }

    #[test]
    /// UI-R-378 — every context lists `C_Time`, `C_Test`, `C_Log` and `print` in full.
    fn ut_every_context_lists_shared_modules() {
        for b in every_context() {
            let secs = sections(&b);
            assert_lists(section(&secs, "C_Time"), docs::TIME);
            assert_lists(section(&secs, "C_Test"), docs::TEST);
            assert_lists(section(&secs, "C_Log"), docs::LOG);
            let print = section(&secs, "print");
            assert!(
                print
                    .entries
                    .iter()
                    .any(|(s, _)| s.starts_with(docs::PRINT.name)),
                "{:?} misses print",
                b.context()
            );
        }
    }

    #[test]
    /// UI-R-379 — each OCPP page lists only its own role's accessor shape.
    fn ut_ocpp_client_lists_own_shape_only() {
        let secs = sections(&client16());
        let ocpp = section(&secs, "C_OCPP");
        for name in ["Connector", "GetConnectors", "ConnectionStatus"] {
            assert!(
                ocpp.entries.iter().any(|(s, _)| s.contains(name)),
                "client misses {name}"
            );
        }
        for (sig, _) in &ocpp.entries {
            assert!(!sig.contains("GetChargingStations"), "{sig}");
            assert!(!sig.contains("ChargingStation("), "{sig}");
        }

        let secs = sections(&server16());
        let ocpp = section(&secs, "C_OCPP");
        assert_lists(ocpp, docs::OCPP_SERVER);
        for (sig, _) in &ocpp.entries {
            assert!(!sig.contains("Connector(id: integer)"), "{sig}");
        }
    }

    #[test]
    /// UI-R-382 — the client page lists one entry per CS and connector action of each version.
    fn ut_ocpp_client_lists_every_version_action() {
        for (b, cs, conn) in [
            (client16(), Cs16::cs_actions(), Cs16::conn_actions()),
            (client201(), Cs201::cs_actions(), Cs201::conn_actions()),
        ] {
            let secs = sections(&b);
            assert!(!cs.is_empty() && !conn.is_empty());
            assert_names(section(&secs, "C_OCPP"), &cs);
            assert_names(section(&secs, "C_OCPP:Connector(id)"), &conn);
        }
    }

    #[test]
    /// UI-R-382 — the server page lists one entry per station and connector action of each version.
    fn ut_ocpp_server_lists_every_version_action() {
        for (b, actions) in [
            (server16(), server_actions::<V1_6>()),
            (server201(), server_actions::<V2_0_1>()),
        ] {
            let (station, conn) = actions;
            let secs = sections(&b);
            assert!(!station.is_empty() && !conn.is_empty());
            assert_names(section(&secs, "C_OCPP:ChargingStation(cs)"), &station);
            assert_names(section(&secs, "C_OCPP:Connector(cs, id)"), &conn);
        }
    }

    #[test]
    /// UI-R-380 — the session page lists the module directory, handle and accessor surfaces.
    fn ut_session_lists_module_and_accessor_surfaces() {
        let secs = sections(&LuaBindings::Session);
        assert_lists(section(&secs, "C_Module"), docs::MODULE_DIR);
        assert_lists(section(&secs, "ModuleHandle"), docs::MODULE_HANDLE);
        assert_lists(section(&secs, "Register() accessor"), docs::REGISTER);
        let client = section(&secs, "OCPP() accessor, client module");
        assert_lists(client, docs::OCPP_CLIENT);
        assert_lists(client, docs::OCPP_STATE);
        assert_lists(
            section(&secs, "OCPP() accessor, server module"),
            docs::OCPP_SERVER,
        );
        assert_lists(section(&secs, "OCPP scope accessor"), docs::OCPP_STATE);
    }

    #[test]
    /// UI-R-383 — the session page carries exactly one generic `<Action>` entry and no concrete action.
    fn ut_session_lists_one_generic_action() {
        let secs = sections(&LuaBindings::Session);
        let generic: Vec<&(String, String)> = secs
            .iter()
            .flat_map(|s| s.entries.iter())
            .filter(|(sig, _)| sig.contains("<Action>"))
            .collect();
        assert_eq!(generic.len(), 1);
        assert!(generic[0].1.contains("version"));
        for (sig, _) in secs.iter().flat_map(|s| s.entries.iter()) {
            assert!(!sig.contains("Heartbeat"), "{sig}");
            assert!(!sig.contains("MeterValues"), "{sig}");
        }
    }

    #[test]
    /// UI-R-381 — every entry shows a typed signature: `) -> ` and `name: type` for each argument.
    fn ut_every_entry_shows_types() {
        for b in every_context() {
            let secs = sections(&b);
            let all: Vec<_> = secs.iter().flat_map(|s| s.entries.iter()).collect();
            assert!(!all.is_empty());
            for (sig, _) in all {
                assert!(sig.contains(") -> "), "{sig}");
                let open = sig.find('(').unwrap();
                let close = sig.rfind(") -> ").unwrap();
                let args = &sig[open + 1..close];
                if args.is_empty() {
                    continue;
                }
                for arg in args.split(", ") {
                    assert!(arg.contains(": "), "untyped arg '{arg}' in {sig}");
                }
            }
        }
    }

    #[test]
    /// UI-E-173 — the help lists no state field names, current or former.
    fn ut_help_lists_no_field_names() {
        for b in every_context() {
            let secs = sections(&b);
            let all: Vec<_> = secs.iter().flat_map(|s| s.entries.iter()).collect();
            assert!(!all.is_empty());
            for (sig, desc) in all {
                for banned in [
                    "Session Energy",
                    "Firmware Version",
                    "Current L1",
                    "SessionEnergy",
                ] {
                    assert!(!sig.contains(banned) && !desc.contains(banned), "{sig}");
                }
            }
        }
    }
}
