//! The bundled Lua script templates (SC-R-036): starting points a user can drop into a module's
//! script list from the script dialog's template browser.
//!
//! A template is *not* a script: its code is compiled into the binary and only ever **copied** into
//! a [`ScriptDef`](crate::config::script::ScriptDef) — nothing reads template code from disk at run
//! time, so scripts stay stored inline in the config files (SC-R-023).

use crate::dialog::lua_help::ScriptContext;

/// One bundled template: its display name, a one-line description, the script contexts it applies
/// to, and its Lua code.
#[derive(Debug, PartialEq, Eq)]
pub struct ScriptTemplate {
    pub name: &'static str,
    pub description: &'static str,
    pub contexts: &'static [ScriptContext],
    pub code: &'static str,
}

// The `TEMPLATES` array is generated at build time by `build.rs`, which walks `templates/` and
// derives each entry's name (file stem), context (parent directory), description (leading
// `-- description:` header), and code (the file minus that header). See SC-R-036.
include!(concat!(env!("OUT_DIR"), "/templates_generated.rs"));

/// The templates applicable to `ctx`, in declaration order.
pub fn templates(ctx: ScriptContext) -> Vec<&'static ScriptTemplate> {
    TEMPLATES
        .iter()
        .filter(|t| t.contexts.contains(&ctx))
        .collect()
}

/// The bundled session template the `--demo` session script is built from.
pub fn by_name(name: &str) -> Option<&'static ScriptTemplate> {
    TEMPLATES.iter().find(|t| t.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTEXTS: [ScriptContext; 4] = [
        ScriptContext::Modbus,
        ScriptContext::OcppClient,
        ScriptContext::OcppServer,
        ScriptContext::Session,
    ];

    #[test]
    /// SC-R-037 — every bundled template's code body loads in the Lua runtime; a syntax error in a
    /// template is a test failure, never something a user meets after inserting it.
    fn ut_every_template_compiles() {
        for template in TEMPLATES {
            let ctx = ferrowl_lua::ContextBuilder::<String>::default()
                .with_stdlib()
                .with_script(template.name.to_string(), template.code)
                .build();
            assert!(
                ctx.is_ok(),
                "template '{}' failed to load: {:?}",
                template.name,
                ctx.err()
            );
        }
    }

    #[test]
    /// SC-R-036 — `templates(ctx)` returns exactly the templates declaring `ctx`.
    fn ut_templates_filtered_by_context() {
        for ctx in CONTEXTS {
            for template in templates(ctx) {
                assert!(
                    template.contexts.contains(&ctx),
                    "'{}' listed under {ctx:?} without declaring it",
                    template.name
                );
            }
        }
        let modbus: Vec<_> = templates(ScriptContext::Modbus)
            .iter()
            .map(|t| t.name)
            .collect();
        assert!(modbus.contains(&"register-ramp"));
        assert!(!modbus.contains(&"power-report"));
    }

    #[test]
    /// SC-R-036 — every script context has at least one template, so the browser is never empty.
    fn ut_every_context_has_a_template() {
        for ctx in CONTEXTS {
            assert!(!templates(ctx).is_empty(), "{ctx:?} has no template");
        }
    }

    #[test]
    fn ut_by_name_finds_bundled_template() {
        assert!(by_name("power-report").is_some());
        assert!(by_name("nope").is_none());
    }
}
