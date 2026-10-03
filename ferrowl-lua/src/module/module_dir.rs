//! Lua module `C_Module`: session-level access to every host module's Lua surface, resolved by
//! name against a live [`ModuleDirectory`].
//!
//! Unlike the other host modules (which wrap a single fixed handle), `C_Module` is a lookup: it
//! hands scripts a [`ModuleHandle`] that re-resolves its target through the directory on every
//! call. That makes the surface immediately consistent with modules being added or removed at
//! runtime — a handle obtained before a removal simply starts erroring afterwards instead of
//! reading stale state.

use crate::module::{Has, OcppGuard, Read, RegisterModule, ValueType, Write};
use ferrowl_lua_derive::Module;
use mlua::{AnyUserData, Lua, Result, UserData, UserDataMethods};
use std::sync::Arc;

/// Register read/write/has access of a modbus module, type-erased.
pub trait RegisterAccess: Read + Write + Has + Send + Sync {}
impl<T: Read + Write + Has + Send + Sync> RegisterAccess for T {}

/// One module's Lua-facing surface, type-erased. Implementations capture only `Send + Sync`
/// shared state (e.g. `Arc`s) so a [`ModuleDirectory`] can be built and resolved independently of
/// any particular Lua context.
pub trait ModuleHost: Send + Sync {
    /// The module kind, e.g. `"modbus"` or `"ocpp"`.
    fn kind(&self) -> &'static str;
    /// The module's role, e.g. `"client"` or `"server"`.
    fn role(&self) -> &'static str;
    /// Identity of this module instance behind its name: stable across registry rebuilds of one
    /// instance, different for a replacement (SC-R-080).
    fn instance_id(&self) -> u64;
    /// Register access for a modbus module, `None` for any other kind.
    fn register_access(&self) -> Option<Arc<dyn RegisterAccess>>;
    /// Builds the role-shaped `C_OCPP` accessor userdata for an ocpp module, running `guard`
    /// before every method; `Ok(None)` for any other kind.
    fn ocpp_accessor(&self, lua: &Lua, guard: OcppGuard) -> Result<Option<AnyUserData>>;
}

/// Live directory of modules, resolved by name at every access rather than snapshotted once.
pub trait ModuleDirectory: Send + Sync {
    /// Names of every currently known module.
    fn list(&self) -> Vec<String>;
    /// Resolves `name` to its host, `None` if no such module currently exists.
    fn resolve(&self, name: &str) -> Option<Arc<dyn ModuleHost>>;
}

/// Lua module `C_Module`: enumerates and resolves the session's other host modules by name.
///
/// Exposed Lua methods: `List()` — sorted array of module names — and `Get(name)`, which raises
/// if `name` is unknown and otherwise returns a [`ModuleHandle`].
#[derive(Module)]
#[module = "C_Module"]
pub struct ModuleDir {
    directory: Arc<dyn ModuleDirectory>,
}

impl ModuleDir {
    /// Creates the module around the host's live module directory.
    pub fn init(directory: Arc<dyn ModuleDirectory>) -> Self {
        Self { directory }
    }
}

impl UserData for ModuleDir {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("List", |_, this, ()| {
            let mut names = this.directory.list();
            names.sort();
            Ok(names)
        });
        methods.add_method("Get", |_, this, name: String| {
            if this.directory.resolve(&name).is_none() {
                return Err(mlua::Error::RuntimeError(format!(
                    "unknown module '{name}'"
                )));
            }
            Ok(ModuleHandle {
                directory: this.directory.clone(),
                name,
            })
        });
    }
}

/// `C_Register` access that re-resolves the module by name on every call, so a rebuilt or
/// replaced modbus module is reached without a fresh `Register()` call (SC-R-078).
struct LiveRegister {
    directory: Arc<dyn ModuleDirectory>,
    name: String,
}

impl LiveRegister {
    fn access(&self) -> Result<Arc<dyn RegisterAccess>> {
        let host = self
            .directory
            .resolve(&self.name)
            .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown module '{}'", self.name)))?;
        host.register_access().ok_or_else(|| {
            mlua::Error::RuntimeError(format!("module '{}' is not a modbus module", self.name))
        })
    }
}

impl Read for LiveRegister {
    fn read(&self, name: String) -> Result<ValueType> {
        self.access()?.read(name)
    }
}

impl Write for LiveRegister {
    fn write(&self, name: String, value: ValueType) -> Result<()> {
        self.access()?.write(name, value)
    }
}

impl Has for LiveRegister {
    fn has(&self, name: String) -> Result<bool> {
        self.access()?.has(name)
    }
}

/// A reference to one module resolved by name. Every method re-resolves the name through the
/// directory, so a module removed after `Get` surfaces as an "unknown module" error rather than
/// returning stale data. A `Register()` accessor re-resolves on every call too (SC-R-078); an
/// `OCPP()` accessor raises `was replaced` once the instance behind the name changes
/// (SC-R-080).
pub struct ModuleHandle {
    directory: Arc<dyn ModuleDirectory>,
    name: String,
}

impl ModuleHandle {
    /// Re-resolves the target host, raising if it no longer exists.
    fn resolve(&self) -> Result<Arc<dyn ModuleHost>> {
        self.directory
            .resolve(&self.name)
            .ok_or_else(|| mlua::Error::RuntimeError(format!("unknown module '{}'", self.name)))
    }
}

impl UserData for ModuleHandle {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("Type", |_, this, ()| Ok(this.resolve()?.kind()));
        methods.add_method("Role", |_, this, ()| Ok(this.resolve()?.role()));
        methods.add_method("Register", |lua, this, ()| {
            let host = this.resolve()?;
            if host.register_access().is_none() {
                return Err(mlua::Error::RuntimeError(format!(
                    "module '{}' is not a modbus module",
                    this.name
                )));
            }
            lua.create_userdata(RegisterModule::init(LiveRegister {
                directory: this.directory.clone(),
                name: this.name.clone(),
            }))
        });
        methods.add_method("OCPP", |lua, this, ()| {
            let host = this.resolve()?;
            let id = host.instance_id();
            let dir = this.directory.clone();
            let name = this.name.clone();
            let guard = OcppGuard::new(move || match dir.resolve(&name) {
                Some(h) if h.instance_id() == id => Ok(()),
                _ => Err(mlua::Error::RuntimeError(format!(
                    "module '{name}' was replaced; call OCPP() again"
                ))),
            });
            host.ocpp_accessor(lua, guard)?.ok_or_else(|| {
                mlua::Error::RuntimeError(format!("module '{}' is not an ocpp module", this.name))
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ContextBuilder;
    use crate::module::OcppClient;
    use crate::module::ValueType;
    use crate::module::test_support::MockHost;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// Directory backed by a map the test can mutate directly (to simulate module removal)
    /// independently of the `Arc<dyn ModuleDirectory>` handed to the Lua context.
    #[derive(Default)]
    struct MockDirectory {
        modules: RwLock<HashMap<String, Arc<dyn ModuleHost>>>,
    }

    impl MockDirectory {
        fn insert(&self, name: &str, host: Arc<dyn ModuleHost>) {
            self.modules.write().unwrap().insert(name.to_string(), host);
        }
        fn remove(&self, name: &str) {
            self.modules.write().unwrap().remove(name);
        }
    }

    impl ModuleDirectory for MockDirectory {
        fn list(&self) -> Vec<String> {
            self.modules.read().unwrap().keys().cloned().collect()
        }
        fn resolve(&self, name: &str) -> Option<Arc<dyn ModuleHost>> {
            self.modules.read().unwrap().get(name).cloned()
        }
    }

    /// A minimal modbus-shaped host: kind `"modbus"`, no ocpp accessor.
    struct ModbusHost {
        role: &'static str,
        rw: MockHost,
        id: u64,
    }
    impl ModuleHost for ModbusHost {
        fn kind(&self) -> &'static str {
            "modbus"
        }
        fn role(&self) -> &'static str {
            self.role
        }
        fn instance_id(&self) -> u64 {
            self.id
        }
        fn register_access(&self) -> Option<Arc<dyn RegisterAccess>> {
            Some(Arc::new(self.rw.clone()))
        }
        fn ocpp_accessor(&self, _lua: &Lua, _guard: OcppGuard) -> Result<Option<AnyUserData>> {
            Ok(None)
        }
    }

    /// A minimal ocpp-shaped host: kind `"ocpp"`, no register accessor.
    struct OcppHost {
        role: &'static str,
        handle: MockHost,
        id: u64,
    }
    impl ModuleHost for OcppHost {
        fn kind(&self) -> &'static str {
            "ocpp"
        }
        fn role(&self) -> &'static str {
            self.role
        }
        fn instance_id(&self) -> u64 {
            self.id
        }
        fn register_access(&self) -> Option<Arc<dyn RegisterAccess>> {
            None
        }
        fn ocpp_accessor(&self, lua: &Lua, guard: OcppGuard) -> Result<Option<AnyUserData>> {
            Ok(Some(lua.create_userdata(OcppClient::init_guarded(
                self.handle.clone(),
                guard,
            ))?))
        }
    }

    #[test]
    /// SC-R-020 — C_Module enumerates the session's other modules by name.
    fn ut_list_returns_sorted_names() {
        let dir = Arc::new(MockDirectory::default());
        dir.insert(
            "b",
            Arc::new(ModbusHost {
                role: "client",
                rw: MockHost::default(),
                id: 0,
            }),
        );
        dir.insert(
            "a",
            Arc::new(ModbusHost {
                role: "client",
                rw: MockHost::default(),
                id: 0,
            }),
        );
        let module = ModuleDir::init(dir as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script(
                "s".to_string(),
                r#"
                names = C_Module:List()
                "#,
            )
            .build()
            .expect("build context");
        ctx.call_all().expect("run");
    }

    #[test]
    /// SC-R-020 — resolving an unknown module name raises rather than returning a null handle.
    fn ut_get_unknown_module_raises() {
        let dir = Arc::new(MockDirectory::default());
        let module = ModuleDir::init(dir as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script("s".to_string(), r#"C_Module:Get("nope")"#)
            .build()
            .expect("build context");
        let err = ctx.call_all().unwrap_err();
        assert!(err[0].to_string().contains("unknown module"));
    }

    #[test]
    /// SC-R-020 — a resolved C_Module handle reports its target's kind and role.
    fn ut_type_and_role_return_host_values() {
        let dir = Arc::new(MockDirectory::default());
        dir.insert(
            "a",
            Arc::new(ModbusHost {
                role: "server",
                rw: MockHost::default(),
                id: 0,
            }),
        );
        let module = ModuleDir::init(dir as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script(
                "s".to_string(),
                r#"
                local m = C_Module:Get("a")
                kind = m:Type()
                role = m:Role()
                "#,
            )
            .build()
            .expect("build context");
        ctx.call_all().expect("run");
    }

    #[test]
    /// SC-R-020 — C_Module hands out a C_Register-shaped accessor only for a modbus module; others raise.
    fn ut_register_on_non_modbus_raises() {
        let dir = Arc::new(MockDirectory::default());
        dir.insert(
            "a",
            Arc::new(OcppHost {
                role: "client",
                handle: MockHost::default(),
                id: 0,
            }),
        );
        let module = ModuleDir::init(dir as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script(
                "s".to_string(),
                r#"local m = C_Module:Get("a"); m:Register()"#,
            )
            .build()
            .expect("build context");
        let err = ctx.call_all().unwrap_err();
        assert!(err[0].to_string().contains("is not a modbus module"));
    }

    #[test]
    /// SC-R-020 — C_Module hands out a C_OCPP-shaped accessor only for an ocpp module; others raise.
    fn ut_ocpp_on_non_ocpp_raises() {
        let dir = Arc::new(MockDirectory::default());
        dir.insert(
            "a",
            Arc::new(ModbusHost {
                role: "client",
                rw: MockHost::default(),
                id: 0,
            }),
        );
        let module = ModuleDir::init(dir as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script("s".to_string(), r#"local m = C_Module:Get("a"); m:OCPP()"#)
            .build()
            .expect("build context");
        let err = ctx.call_all().unwrap_err();
        assert!(err[0].to_string().contains("is not an ocpp module"));
    }

    #[test]
    /// SC-R-020 — the session sim reaches a modbus module's register state through C_Module.
    fn ut_register_roundtrip_through_directory() {
        let rw = MockHost::default();
        let dir = Arc::new(MockDirectory::default());
        dir.insert(
            "a",
            Arc::new(ModbusHost {
                role: "client",
                rw: rw.clone(),
                id: 0,
            }),
        );
        let module = ModuleDir::init(dir as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script(
                "s".to_string(),
                r#"
                local m = C_Module:Get("a")
                m:Register():Set("x", 7)
                v = m:Register():Get("x")
                "#,
            )
            .build()
            .expect("build context");
        ctx.call_all().expect("run");

        match rw.get("x") {
            Some(ValueType::Int(v)) => assert_eq!(v, 7),
            other => panic!("expected Int(7), got {other:?}"),
        }
    }

    #[test]
    /// SC-R-020 — the session sim reaches an ocpp module's state and actions through C_Module.
    fn ut_ocpp_roundtrip_through_directory() {
        let handle = MockHost::default();
        let dir = Arc::new(MockDirectory::default());
        dir.insert(
            "a",
            Arc::new(OcppHost {
                role: "client",
                handle: handle.clone(),
                id: 0,
            }),
        );
        let module = ModuleDir::init(dir as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script(
                "s".to_string(),
                r#"
                local m = C_Module:Get("a")
                m:OCPP():Set("Model", "M")
                m:OCPP():Connector(1):Set("Power", 11)
                m:OCPP():BootNotification()
                "#,
            )
            .build()
            .expect("build context");
        ctx.call_all().expect("run");

        assert!(matches!(
            handle.get("Model"),
            Some(ValueType::String(s)) if s == "M"
        ));
        let conn1 = handle.conn_store(1);
        assert!(matches!(
            conn1.lock().unwrap().get("Power"),
            Some(ValueType::Int(11))
        ));
        assert!(
            handle
                .dispatched_pairs()
                .contains(&("".to_string(), "BootNotification".to_string()))
        );
    }

    #[test]
    /// SC-R-020 — a C_Module handle re-resolves by name each call, so a removed module goes stale.
    fn ut_handle_becomes_stale_after_removal() {
        let dir = Arc::new(MockDirectory::default());
        dir.insert(
            "a",
            Arc::new(ModbusHost {
                role: "client",
                rw: MockHost::default(),
                id: 0,
            }),
        );
        let module = ModuleDir::init(dir.clone() as Arc<dyn ModuleDirectory>);
        let mut ctx = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(module)
            .with_script("get".to_string(), r#"m = C_Module:Get("a")"#)
            .with_script("probe".to_string(), r#"kind = m:Type()"#)
            .build()
            .expect("build context");

        // The handle resolves fine while "a" is present.
        ctx.call(&"get".to_string()).expect("get succeeds");
        ctx.call(&"probe".to_string())
            .expect("probe succeeds before removal");

        // Removing the module from the directory makes the already-held handle stale.
        dir.remove("a");
        let err = ctx.call(&"probe".to_string()).unwrap_err();
        assert!(err.to_string().contains("unknown module"));
    }
    fn ctx_with(dir: &Arc<MockDirectory>, scripts: &[(&str, &str)]) -> crate::Context<String> {
        let mut builder = ContextBuilder::<String>::default()
            .with_stdlib()
            .with_module(ModuleDir::init(dir.clone() as Arc<dyn ModuleDirectory>));
        for (name, code) in scripts {
            builder = builder.with_script(name.to_string(), code);
        }
        builder.build().expect("build context")
    }

    fn modbus(rw: &MockHost, id: u64) -> Arc<ModbusHost> {
        Arc::new(ModbusHost {
            role: "client",
            rw: rw.clone(),
            id,
        })
    }

    fn ocpp(handle: &MockHost, id: u64) -> Arc<OcppHost> {
        Arc::new(OcppHost {
            role: "client",
            handle: handle.clone(),
            id,
        })
    }

    const REPLACED: &str = "module 'cs' was replaced; call OCPP() again";

    #[test]
    /// SC-R-077 — a module added after the context was built is returned by the next `Get`.
    fn ut_get_resolves_module_added_after_context_built() {
        let dir = Arc::new(MockDirectory::default());
        let mut ctx = ctx_with(
            &dir,
            &[(
                "s",
                r#"local m = C_Module:Get("late"); m:Register():Set("kind", m:Type())"#,
            )],
        );
        let rw = MockHost::default();
        dir.insert("late", modbus(&rw, 1));
        ctx.call_all().expect("late module resolves");
        assert!(matches!(rw.get("kind"), Some(ValueType::String(s)) if s == "modbus"));
    }

    #[test]
    /// SC-R-078 — a held `Register()` accessor reaches the module that replaced its host.
    fn ut_held_register_accessor_follows_replaced_host() {
        let dir = Arc::new(MockDirectory::default());
        let old = MockHost::default();
        old.store
            .lock()
            .unwrap()
            .insert("x".into(), ValueType::Int(1));
        dir.insert("m", modbus(&old, 1));
        let mut ctx = ctx_with(
            &dir,
            &[
                ("hold", r#"r = C_Module:Get("m"):Register(); r:Has("x")"#),
                ("use", r#"r:Set("out", r:Get("x"))"#),
            ],
        );
        ctx.call(&"hold".to_string()).expect("hold");
        let new = MockHost::default();
        new.store
            .lock()
            .unwrap()
            .insert("x".into(), ValueType::Int(2));
        dir.insert("m", modbus(&new, 2));
        ctx.call(&"use".to_string()).expect("use");
        assert!(matches!(new.get("out"), Some(ValueType::Int(2))));
        assert!(old.get("out").is_none());
    }

    #[test]
    /// SC-R-080 — held `OCPP()` accessors (and derived ones) raise once the instance behind the name changes or is closed.
    fn ut_held_ocpp_accessor_raises_replaced_after_id_change() {
        let dir = Arc::new(MockDirectory::default());
        let handle = MockHost::default();
        dir.insert("cs", ocpp(&handle, 1));
        let mut ctx = ctx_with(
            &dir,
            &[
                (
                    "hold",
                    r#"o = C_Module:Get("cs"):OCPP(); c = o:Connector(1)"#,
                ),
                ("get", r#"o:Get("Model")"#),
                ("list", r#"o:GetConnectors()"#),
                ("conn", r#"c:Get("Power")"#),
                ("fresh", r#"C_Module:Get("cs"):OCPP():Set("Model", "M")"#),
                ("gone", r#"C_Module:Get("cs")"#),
            ],
        );
        ctx.call(&"hold".to_string()).expect("hold");
        dir.insert("cs", ocpp(&MockHost::default(), 2));
        for name in ["get", "list", "conn"] {
            let err = ctx.call(&name.to_string()).unwrap_err();
            assert!(err.to_string().contains(REPLACED), "{name}: {err}");
        }
        ctx.call(&"fresh".to_string())
            .expect("fresh accessor works");

        dir.remove("cs");
        let err = ctx.call(&"get".to_string()).unwrap_err();
        assert!(err.to_string().contains(REPLACED), "{err}");
        let err = ctx.call(&"gone".to_string()).unwrap_err();
        assert!(err.to_string().contains("unknown module 'cs'"), "{err}");
    }

    #[test]
    /// SC-R-078 — a held `OCPP()` accessor follows a rebuilt host object that keeps the instance id.
    fn ut_held_ocpp_accessor_follows_same_instance() {
        let dir = Arc::new(MockDirectory::default());
        let handle = MockHost::default();
        dir.insert("cs", ocpp(&handle, 7));
        let mut ctx = ctx_with(
            &dir,
            &[
                ("hold", r#"o = C_Module:Get("cs"):OCPP()"#),
                ("set", r#"o:Set("Model", "M")"#),
            ],
        );
        ctx.call(&"hold".to_string()).expect("hold");
        dir.insert("cs", ocpp(&handle, 7));
        ctx.call(&"set".to_string()).expect("held accessor follows");
        assert!(matches!(handle.get("Model"), Some(ValueType::String(s)) if s == "M"));
    }
}
