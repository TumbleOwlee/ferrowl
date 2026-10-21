//! Typed method documentation for every host module, the single source the Lua-bindings help
//! overlay renders. Each module keeps its table beside its method registrations; the tables are
//! re-exported here by role.

/// One documented argument.
pub struct ArgDoc {
    pub name: &'static str,
    pub ty: &'static str,
    pub optional: bool,
}

/// One documented method.
pub struct MethodDoc {
    /// `"Get"`, or `"<Action>"` for the action template.
    pub name: &'static str,
    pub args: &'static [ArgDoc],
    /// `"number|string|boolean|nil"`; `"nil"` for no value.
    pub returns: &'static str,
    pub summary: &'static str,
}

impl MethodDoc {
    /// `"<receiver>:Name(arg: type, opt?: type) -> ret"`; an empty receiver renders no prefix.
    pub fn signature(&self, receiver: &str) -> String {
        self.render(receiver, self.name)
    }

    /// Same shape for one concrete action name, from the `<Action>` template.
    pub fn action_signature(&self, receiver: &str, action: &str) -> String {
        self.render(receiver, action)
    }

    fn render(&self, receiver: &str, name: &str) -> String {
        let args = self
            .args
            .iter()
            .map(|a| {
                let opt = if a.optional { "?" } else { "" };
                format!("{}{opt}: {}", a.name, a.ty)
            })
            .collect::<Vec<_>>()
            .join(", ");
        let prefix = if receiver.is_empty() {
            String::new()
        } else {
            format!("{receiver}:")
        };
        format!("{prefix}{name}({args}) -> {}", self.returns)
    }
}

pub(crate) const fn arg(name: &'static str, ty: &'static str) -> ArgDoc {
    ArgDoc {
        name,
        ty,
        optional: false,
    }
}

pub(crate) const fn method(
    name: &'static str,
    args: &'static [ArgDoc],
    returns: &'static str,
    summary: &'static str,
) -> MethodDoc {
    MethodDoc {
        name,
        args,
        returns,
        summary,
    }
}

pub(crate) const CONNECTION_STATUS: MethodDoc = MethodDoc {
    name: "ConnectionStatus",
    args: &[],
    returns: "string",
    summary: "module connection status: \"CONNECTED\", \"RECONNECTING\" or \"DISCONNECTED\"",
};

pub const REGISTER: &[MethodDoc] = crate::module::register::DOCS;
pub const OCPP_STATE: &[MethodDoc] = crate::module::ocpp::STATE_DOCS;
pub const OCPP_ACTION: MethodDoc = crate::module::ocpp::ACTION_DOC;
pub const OCPP_FLAT: &[MethodDoc] = crate::module::ocpp::FLAT_DOCS;
pub const OCPP_CLIENT: &[MethodDoc] = crate::module::ocpp::CLIENT_DOCS;
pub const OCPP_SERVER: &[MethodDoc] = crate::module::ocpp::SERVER_DOCS;
pub const MODULE_DIR: &[MethodDoc] = crate::module::module_dir::DIR_DOCS;
pub const MODULE_HANDLE: &[MethodDoc] = crate::module::module_dir::HANDLE_DOCS;
pub const TIME: &[MethodDoc] = crate::module::time::DOCS;
pub const TEST: &[MethodDoc] = crate::module::test::DOCS;
pub const LOG: &[MethodDoc] = crate::module::log::DOCS;

pub const PRINT: MethodDoc = MethodDoc {
    name: "print",
    args: &[ArgDoc {
        name: "...",
        ty: "any",
        optional: false,
    }],
    returns: "nil",
    summary: "redirected to the module log, like C_Log:Info",
};

#[cfg(test)]
mod tests {
    use super::*;

    fn all_tables() -> Vec<&'static [MethodDoc]> {
        vec![
            REGISTER,
            OCPP_STATE,
            OCPP_FLAT,
            OCPP_CLIENT,
            OCPP_SERVER,
            MODULE_DIR,
            MODULE_HANDLE,
            TIME,
            TEST,
            LOG,
        ]
    }

    #[test]
    fn ut_signature_renders_typed_args_and_return() {
        let set = REGISTER.iter().find(|m| m.name == "Set").unwrap();
        assert_eq!(
            set.signature("C_Register"),
            "C_Register:Set(name: string, value: number|string|boolean) -> nil"
        );
        assert!(
            OCPP_ACTION
                .signature("C_OCPP")
                .contains("(overrides?: table)")
        );
        assert_eq!(PRINT.signature(""), "print(...: any) -> nil");
    }

    #[test]
    fn ut_action_signature_names_the_action() {
        assert_eq!(
            OCPP_ACTION.action_signature("C_OCPP", "Heartbeat"),
            "C_OCPP:Heartbeat(overrides?: table) -> boolean"
        );
    }

    #[test]
    fn ut_every_doc_entry_is_typed() {
        let mut entries: Vec<&MethodDoc> = all_tables().into_iter().flatten().collect();
        entries.push(&OCPP_ACTION);
        entries.push(&PRINT);
        for m in entries {
            assert!(!m.returns.is_empty(), "{} has no return type", m.name);
            assert!(!m.summary.is_empty(), "{} has no summary", m.name);
            for a in m.args {
                assert!(!a.ty.is_empty(), "{}({}) has no type", m.name, a.name);
            }
        }
    }
}
