use crate::{Context, Result, module::LogSink, module::Module};
use mlua::UserData;
use std::hash::Hash;
use std::sync::{Arc, atomic::AtomicBool};

/// Lua context builder
pub struct ContextBuilder<K>
where
    K: Hash + Eq + Default,
{
    context: Result<Context<K>>,
    stop_flag: Option<Arc<AtomicBool>>,
}

impl<K> Default for ContextBuilder<K>
where
    K: Hash + Eq + Default,
{
    /// Create new context builder
    fn default() -> Self {
        Self {
            context: Ok(Context::<K>::default()),
            stop_flag: None,
        }
    }
}

impl<K> ContextBuilder<K>
where
    K: Hash + Eq + Default,
{
    /// Create context builder from context result. Test-only: production builders start from
    /// [`ContextBuilder::default`] and chain `with_*`.
    #[cfg(test)]
    fn from(context: Result<Context<K>>) -> Self {
        Self {
            context,
            stop_flag: None,
        }
    }

    /// SC-R-034 / SC-R-012 — attach a sim thread's stop flag so the execution hook installed in
    /// `build()` can unwind a runaway script promptly instead of only observing the flag between
    /// cycles. Only a sim-thread call site passes this; an on-demand run (SC-R-035) omits it.
    pub fn with_stop_flag(mut self, stop: Arc<AtomicBool>) -> Self {
        self.stop_flag = Some(stop);
        self
    }

    /// Add a new module to the lua context
    pub fn with_module<T>(mut self, value: T) -> Self
    where
        T: 'static + Module + UserData,
    {
        if let Ok(ref mut ctx) = self.context
            && let Err(e) = ctx.add_module(value)
        {
            self.context = Err(e);
        }
        self
    }

    /// Enable support of standard libraries in lua context
    pub fn with_stdlib(mut self) -> Self {
        if let Ok(ref mut ctx) = self.context
            && let Err(e) = ctx.enable_stdlib()
        {
            self.context = Err(e);
        }
        self
    }

    /// Redirect the global `print` to a host log sink. Order relative to `with_stdlib()` does not
    /// matter in practice (enabling the standard libraries does not reload `base`/`print`), but
    /// call this after `with_stdlib()` to keep the builder chain reading top-to-bottom as setup
    /// followed by overrides.
    pub fn with_print_sink<S>(mut self, sink: S) -> Self
    where
        S: LogSink + 'static,
    {
        if let Ok(ref mut ctx) = self.context
            && let Err(e) = ctx.redirect_print(sink)
        {
            self.context = Err(e);
        }
        self
    }

    ///  Load a given script into the lua context and store it under the given key
    pub fn with_script(mut self, key: K, script: &str) -> Self {
        if let Ok(ref mut ctx) = self.context
            && let Err(e) = ctx.load_script(key, script)
        {
            self.context = Err(e);
        }
        self
    }

    /// Build the final context, installing the SC-R-034 execution hook (wall-clock cap always,
    /// stop-flag check when `with_stop_flag` was called) as the last step.
    pub fn build(self) -> Result<Context<K>> {
        let mut ctx = self.context?;
        ctx.install_execution_hook(self.stop_flag)?;
        Ok(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::ContextBuilder;
    use crate::Context;

    #[test]
    fn ut_builder_from_ok_context() {
        let builder = ContextBuilder::<String>::from(Ok(Context::default()));
        assert!(builder.build().is_ok());
    }

    #[test]
    fn ut_builder_from_err_context_short_circuits() {
        // An already-failed context is carried through unchanged, and further
        // chained calls are no-ops.
        let builder = ContextBuilder::<String>::from(Err(mlua::Error::BindError))
            .with_stdlib()
            .with_script("k".to_string(), "local x = 1");
        assert!(builder.build().is_err());
    }
}
