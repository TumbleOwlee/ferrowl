//! Headless / CI runner: `ferrowl run`.
//!
//! Builds the same [`ModuleView`] instances the TUI's `build_tabs` builds and starts them the
//! same way, but never touches the terminal: it ticks `refresh()` on a timer, drains each
//! module's log to stdout (and optionally a file), and exits with a code that reflects what
//! happened instead of leaving the operator to read a screen.
//!
//! Exit codes: `0` ran to completion (duration elapsed or Ctrl-C), `1` a module's device config
//! failed to load or `start` reported an error, `2` `--exit-on-error` was set and a drained log
//! line looked like a Lua script error.

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ferrowl_lua::module::{ModuleDirectory, ModuleHost};

use crate::app::LogRing;
use crate::cli::RunArgs;
use crate::config::ocpp::OcppRole;
use crate::config::script::ScriptDef;
use crate::config::{self, OcppModuleSpec, OcppSpec};
use crate::module::modbus::ModbusModule as Module;
use crate::module::modbus::view::ModbusModuleView;
use crate::module::ocpp::client::build_client_view;
use crate::module::ocpp::server::build_server_view;
use crate::module::view::{CommandResult, ModuleView, SharedLog};
use crate::registry::{ModuleRegistry, dedupe_names};
use crate::session_sim::SessionSim;
use crate::view::log::format_timestamp;

/// Log source name the session-level Lua sim's drained lines are prefixed with, alongside every
/// module's own name.
const SESSION_SOURCE: &str = "session";

/// How often the loop wakes to refresh modules and drain logs (mirrors `App`'s redraw tick).
const TICK: Duration = Duration::from_millis(100);

/// Ring depth to peek per tick. Matches `crate::app::LOG_SIZE`; kept local so this module has no
/// dependency on the TUI's `App` beyond the shared log type and [`LogRing`] itself.
const LOG_PEEK: usize = 80;

/// Prefix Lua sim script errors are logged under (see `ferrowl/src/lua.rs`). `--exit-on-error`
/// keys off this exact string, so detection is only as good as what actually gets logged.
const SIM_ERROR_PREFIX: &str = "[sim]";

/// One running module: its view (owns start/stop/refresh), display name, log channel, and how
/// many lines of the log have already been drained.
struct RunModule {
    name: String,
    view: Box<dyn ModuleView>,
    log: SharedLog,
    /// Total lines already drained, per [`LogRing::written`]. Draining is exact-by-count rather
    /// than by matching the last-seen line's content: content matching mis-resumes when a
    /// message repeats verbatim within one window (a tight sim loop logging the same error
    /// every tick, e.g.), silently skipping real lines. The only way this can still lose lines
    /// is a full ring-eviction between ticks, which is detected and reported (see [`drain_log`]).
    last_written: u64,
}

/// Build every configured module, starting each one and failing hard (unlike the TUI's
/// `build_tabs`, which skips a bad module with an `eprintln!` and keeps going) if a device config
/// fails to load or `start` reports an error.
async fn build_modules(args: &RunArgs) -> Result<Vec<RunModule>, String> {
    let mut modules = Vec::new();

    for spec in args.module_specs()? {
        let device = config::load_device(&spec.device)
            .map_err(|e| format!("'{}': failed to load '{}': {e}", spec.name, spec.device))?;
        let module = Module::new(&spec, &device);
        let view: Box<dyn ModuleView> =
            Box::new(ModbusModuleView::new(module, spec.clone(), device));
        modules.push(start_module(spec.name.clone(), view).await?);
    }

    for spec in args.ocpp_specs()? {
        modules.push(build_ocpp_module(spec).await?);
    }

    Ok(modules)
}

async fn build_ocpp_module(module: OcppModuleSpec) -> Result<RunModule, String> {
    let name = module.name.clone();
    let device = config::load_ocpp_device(&module.device)
        .map_err(|e| format!("'{name}': failed to load '{}': {e}", module.device))?;
    let spec = OcppSpec::from_parts(&module, &device);
    let view: Box<dyn ModuleView> = match device.role {
        OcppRole::Client => build_client_view(spec, module.device.clone(), device),
        OcppRole::Server => build_server_view(spec, module.device.clone(), device),
    };
    start_module(name, view).await
}

/// Start a module via `handle_command("start")`. The [`ModuleView`] trait has no dedicated
/// success/failure signal for commands (`CommandResult::Handled(Some(msg))` covers both), so an
/// error is detected by the same convention every start handler in this codebase follows: the
/// message contains the substring "failed" (e.g. "Start server failed: ...", "listen failed:
/// ...", "Connect failed: ..."). This is a heuristic, not a structured error channel.
async fn start_module(name: String, mut view: Box<dyn ModuleView>) -> Result<RunModule, String> {
    let log = view.log();
    if let CommandResult::Handled(Some(msg)) = view.handle_command("start").await {
        if msg.to_lowercase().contains("failed") {
            return Err(format!("'{name}': {msg}"));
        }
        log.write().await.write(&msg);
    }
    Ok(RunModule {
        name,
        view,
        log,
        last_written: 0,
    })
}

/// Drain newly-appended lines from one module's log, returning them pre-formatted as
/// `[<timestamp>] <name> | <line>` (the caller just prints/writes them — keeps this testable
/// without capturing stdout). The second return value is `true` when `exit_on_error` is set and
/// one of the drained lines looked like a Lua sim error.
///
/// New-line count is computed exactly from [`LogRing::written`] deltas, not by matching the
/// last-seen line's content — content matching breaks when a message repeats verbatim within one
/// window. The ring is still bounded, though: if more lines were written since the last drain
/// than the ring can hold, the oldest of them are gone for good. That case is reported via a
/// synthetic "lines dropped" line rather than silently under-counted.
async fn drain_log(
    log: &SharedLog,
    name: &str,
    last_written: &mut u64,
    exit_on_error: bool,
) -> (Vec<String>, bool) {
    let (written, window) = {
        let guard = log.read().await;
        (guard.written(), guard.peek_n(LOG_PEEK))
    };

    let new_count = written.saturating_sub(*last_written);
    *last_written = written;

    let mut lines = Vec::new();
    let mut hit_error = false;
    if new_count == 0 {
        return (lines, hit_error);
    }

    if new_count > window.len() as u64 {
        let dropped = new_count - window.len() as u64;
        lines.push(format!(
            "[{}] {name} | ({dropped} lines dropped: ring overflowed between ticks)",
            format_timestamp(now_ms())
        ));
    }

    let take = (new_count as usize).min(window.len());
    let start = window.len() - take;
    for (ts, msg) in &window[start..] {
        lines.push(format!("[{}] {name} | {msg}", format_timestamp(*ts)));
        if exit_on_error && msg.starts_with(SIM_ERROR_PREFIX) {
            hit_error = true;
        }
    }

    (lines, hit_error)
}

/// Build the session-level `C_Module` registry from every running module's
/// [`ModuleView::module_host`], keyed by name deduped the same way [`crate::registry::dedupe_names`]
/// dedupes tab names in the TUI (headless has no tab set of its own, but reuses the same helper so
/// a repeated `--module`/`--ocpp` name, or a session file listing the same name twice, doesn't
/// silently drop one module's host from `C_Module`).
fn build_registry(modules: &[RunModule]) -> ModuleRegistry {
    let names: Vec<String> = modules.iter().map(|m| m.name.clone()).collect();
    let deduped = dedupe_names(&names);

    let mut hosts: HashMap<String, Arc<dyn ModuleHost>> = HashMap::new();
    for (module, name) in modules.iter().zip(deduped.iter()) {
        if let Some(host) = module.view.module_host() {
            hosts.insert(name.clone(), host);
        }
    }

    let registry = ModuleRegistry::new();
    registry.replace_all(hosts);
    registry
}

/// Aggregate every `--session` file's session-level Lua scripts and cycle interval into one
/// config, or `None` when no session file carries any script (including the single-module
/// `--module`/`--ocpp` path, which has no session file at all). Scripts from multiple session
/// files are concatenated in file order; the interval is the last session file's, matching the
/// TUI's `session_sim_config` rule so both entry points resolve multi-file sessions identically.
fn load_session_scripts(args: &RunArgs) -> Result<Option<(Vec<ScriptDef>, Duration)>, String> {
    let mut scripts = Vec::new();
    let mut interval = None;
    for path in &args.sessions {
        let session = config::load_session(path).map_err(|e| e.to_string())?;
        interval = Some(session.interval_duration());
        scripts.extend(session.scripts);
    }
    if scripts.is_empty() {
        return Ok(None);
    }
    Ok(Some((
        scripts,
        interval.unwrap_or(Duration::from_secs_f64(1.0)),
    )))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Stop every module (best-effort: a stop failure is logged but does not change the exit code —
/// we're already tearing down).
async fn stop_all(modules: &mut [RunModule]) {
    for module in modules.iter_mut() {
        if let CommandResult::Handled(Some(msg)) = module.view.handle_command("stop").await {
            module.log.write().await.write(&msg);
        }
    }
}

/// Run the headless session described by `args`. Returns the process exit code; never panics on
/// a module's own runtime errors (those surface as log lines), only on setup failure.
pub async fn run(args: &RunArgs) -> i32 {
    let mut modules = match build_modules(args).await {
        Ok(modules) => modules,
        Err(e) => {
            eprintln!("Error: {e}");
            return 1;
        }
    };

    let mut log_file = match args.log_file.as_deref() {
        Some(path) => match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("Error: failed to open --log-file '{path}': {e}");
                stop_all(&mut modules).await;
                return 1;
            }
        },
        None => None,
    };

    let registry = build_registry(&modules);
    let mut session_sim = match load_session_scripts(args) {
        Ok(Some((scripts, interval))) => {
            let log: SharedLog = Arc::new(tokio::sync::RwLock::new(LogRing::init()));
            let directory: Arc<dyn ModuleDirectory> = Arc::new(registry);
            let mut sim = SessionSim::new(directory, log.clone());
            sim.set_interval(interval);
            sim.set_scripts(scripts);
            Some((sim, log, 0u64))
        }
        Ok(None) => None,
        Err(e) => {
            eprintln!("Error: {e}");
            stop_all(&mut modules).await;
            return 1;
        }
    };

    let deadline = args
        .duration
        .map(|secs| Instant::now() + Duration::from_secs(secs));
    let mut exit_code = 0;

    loop {
        tokio::select! {
            _ = tokio::time::sleep(TICK) => {}
            _ = tokio::signal::ctrl_c() => {
                break;
            }
        }

        for view in modules.iter_mut().map(|m| &mut m.view) {
            view.refresh().await;
        }

        let mut should_stop = false;
        for module in modules.iter_mut() {
            let (lines, hit_error) = drain_log(
                &module.log,
                &module.name,
                &mut module.last_written,
                args.exit_on_error,
            )
            .await;
            for line in &lines {
                println!("{line}");
                if let Some(f) = log_file.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
            }
            if hit_error {
                exit_code = 2;
                should_stop = true;
            }
        }

        if let Some((_, log, last_written)) = session_sim.as_mut() {
            let (lines, hit_error) =
                drain_log(log, SESSION_SOURCE, last_written, args.exit_on_error).await;
            for line in &lines {
                println!("{line}");
                if let Some(f) = log_file.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
            }
            if hit_error {
                exit_code = 2;
                should_stop = true;
            }
        }

        if should_stop {
            break;
        }
        if let Some(deadline) = deadline
            && Instant::now() >= deadline
        {
            break;
        }
    }

    if let Some((sim, ..)) = session_sim.as_mut() {
        sim.stop();
    }
    stop_all(&mut modules).await;
    exit_code
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::LogRing;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    fn new_log() -> SharedLog {
        Arc::new(RwLock::new(LogRing::init()))
    }

    #[tokio::test]
    async fn ut_drain_log_counts_exact_even_with_duplicate_lines() {
        let log = new_log();
        {
            let mut g = log.write().await;
            g.write("dup");
            g.write("dup");
            g.write("dup");
        }
        let mut last_written = 0;
        let (lines, hit) = drain_log(&log, "mod", &mut last_written, false).await;
        assert_eq!(
            lines.len(),
            3,
            "every occurrence of the repeated line must be drained"
        );
        assert!(!hit);
        assert_eq!(last_written, 3);

        // Nothing new since the last drain.
        let (lines, _) = drain_log(&log, "mod", &mut last_written, false).await;
        assert!(lines.is_empty());

        // Two more duplicates land; only those two are new.
        {
            let mut g = log.write().await;
            g.write("dup");
            g.write("dup");
        }
        let (lines, _) = drain_log(&log, "mod", &mut last_written, false).await;
        assert_eq!(lines.len(), 2);
        assert_eq!(last_written, 5);
    }

    #[tokio::test]
    async fn ut_drain_log_reports_dropped_lines_on_ring_overflow() {
        let log = new_log();
        let overflow_by = 5;
        {
            let mut g = log.write().await;
            for i in 0..(LOG_PEEK + overflow_by) {
                g.write(&format!("line {i}"));
            }
        }
        let mut last_written = 0;
        let (lines, _) = drain_log(&log, "mod", &mut last_written, false).await;
        assert_eq!(last_written, (LOG_PEEK + overflow_by) as u64);
        // The full window plus one marker line.
        assert_eq!(lines.len(), LOG_PEEK + 1);
        assert!(lines[0].contains(&format!("{overflow_by} lines dropped")));
    }

    #[tokio::test]
    async fn ut_drain_log_flags_sim_error_prefix_only_when_requested() {
        let log = new_log();
        log.write().await.write("[sim] boom");
        let mut last_written = 0;
        let (_, hit) = drain_log(&log, "mod", &mut last_written, false).await;
        assert!(!hit, "not flagged when --exit-on-error is off");

        let mut last_written = 0;
        let (_, hit) = drain_log(&log, "mod", &mut last_written, true).await;
        assert!(hit);
    }

    #[tokio::test]
    async fn ut_drain_log_session_source_uses_session_prefix() {
        let log = new_log();
        log.write().await.write("hello");
        let mut last_written = 0;
        let (lines, hit) = drain_log(&log, SESSION_SOURCE, &mut last_written, false).await;
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains("session | hello"),
            "unexpected line: {}",
            lines[0]
        );
        assert!(!hit);
    }

    #[tokio::test]
    async fn ut_drain_log_session_sim_error_flags_exit_on_error() {
        let log = new_log();
        log.write().await.write("[sim] boom");
        let mut last_written = 0;
        let (lines, hit) = drain_log(&log, SESSION_SOURCE, &mut last_written, true).await;
        assert!(hit);
        assert!(
            lines[0].contains("session | [sim] boom"),
            "unexpected line: {}",
            lines[0]
        );
    }

    fn empty_run_args(sessions: Vec<String>) -> RunArgs {
        RunArgs {
            sessions,
            modules: vec![],
            ocpp: vec![],
            duration: None,
            log_file: None,
            exit_on_error: false,
        }
    }

    #[test]
    fn ut_load_session_scripts_none_without_session_files() {
        // Mirrors the single-module `--module key=val` headless path (point 5 of the task):
        // no `--session` file means no `Session`, so no session sim is even considered.
        let args = empty_run_args(vec![]);
        assert!(load_session_scripts(&args).unwrap().is_none());
    }

    #[test]
    fn ut_load_session_scripts_aggregates_across_files_last_interval_wins() {
        use crate::config::Session as SessionConfig;
        use ferrowl_util::convert::{Converter, FileType};

        let s1 = SessionConfig {
            version: None,
            modules: vec![],
            scripts: vec![ScriptDef {
                name: "a".into(),
                code: String::new(),
                enabled: true,
            }],
            interval: 2.0,
        };
        let s2 = SessionConfig {
            version: None,
            modules: vec![],
            scripts: vec![ScriptDef {
                name: "b".into(),
                code: String::new(),
                enabled: false,
            }],
            interval: 9.0,
        };
        let p1 = std::env::temp_dir().join("ferrowl_headless_session1.toml");
        let p2 = std::env::temp_dir().join("ferrowl_headless_session2.toml");
        Converter::save(&s1, p1.to_str().unwrap(), FileType::Toml).unwrap();
        Converter::save(&s2, p2.to_str().unwrap(), FileType::Toml).unwrap();

        let args = empty_run_args(vec![
            p1.to_str().unwrap().to_string(),
            p2.to_str().unwrap().to_string(),
        ]);
        let (scripts, interval) = load_session_scripts(&args).unwrap().unwrap();
        assert_eq!(scripts.len(), 2, "scripts from both files are concatenated");
        assert_eq!(
            interval,
            Duration::from_secs_f64(9.0),
            "interval comes from the last session file, matching the TUI rule"
        );
    }

    // --- Integration: a real modbus module + a session-level script talking to it ------------

    fn holding_device_config() -> config::DeviceConfig {
        use crate::module::modbus::config::device::{
            AccessCfg, AlignmentCfg, EndianCfg, RegisterDef, ValueType,
        };
        use ferrowl_codec::Kind;

        let mut definitions = std::collections::BTreeMap::new();
        definitions.insert(
            "value".to_string(),
            RegisterDef {
                slave_id: 1,
                kind: Kind::HoldingRegister,
                address: Some(0),
                is_virtual: false,
                access: AccessCfg::ReadWrite,
                value_type: ValueType::U16,
                endian: EndianCfg::Big,
                resolution: 1.0,
                bitmask: None,
                length: 1,
                alignment: AlignmentCfg::Left,
                values: vec![],
                update: None,
                description: String::new(),
                default: None,
            },
        );
        config::DeviceConfig {
            definitions,
            ..Default::default()
        }
    }

    /// Writes a temp device config + a session file with one modbus module and one session
    /// script, returns a [`RunArgs`] pointing at it. `script_enabled` toggles whether the
    /// session script is enabled, so both the "sim runs" and "zero enabled scripts spawns
    /// nothing" cases share one fixture.
    fn session_run_args(tag: &str, script_enabled: bool) -> RunArgs {
        use ferrowl_util::convert::{Converter, FileType};

        let device_path = std::env::temp_dir().join(format!("ferrowl_headless_{tag}_device.toml"));
        Converter::save(
            &holding_device_config(),
            device_path.to_str().unwrap(),
            FileType::Toml,
        )
        .unwrap();

        let mut module_value = serde_json::to_value(config::ModuleSpec {
            name: "m".to_string(),
            device: device_path.to_str().unwrap().to_string(),
            role: config::Role::Server,
            endpoint: config::Endpoint::Tcp {
                ip: "127.0.0.1".to_string(),
                port: 0,
            },
        })
        .unwrap();
        module_value
            .as_object_mut()
            .unwrap()
            .insert("type".into(), "modbus".into());

        let session = config::Session {
            version: None,
            modules: vec![module_value],
            scripts: vec![ScriptDef {
                name: "s".to_string(),
                code: r#"C_Module:Get("m"):Register():Set("value", 42); C_Log:Print("session-script-ran")"#
                    .to_string(),
                enabled: script_enabled,
            }],
            interval: 0.05,
        };
        let session_path =
            std::env::temp_dir().join(format!("ferrowl_headless_{tag}_session.toml"));
        Converter::save(&session, session_path.to_str().unwrap(), FileType::Toml).unwrap();

        RunArgs {
            sessions: vec![session_path.to_str().unwrap().to_string()],
            modules: vec![],
            ocpp: vec![],
            duration: Some(1),
            log_file: Some(
                std::env::temp_dir()
                    .join(format!("ferrowl_headless_{tag}.log"))
                    .to_str()
                    .unwrap()
                    .to_string(),
            ),
            exit_on_error: false,
        }
    }

    #[tokio::test]
    async fn ut_run_wires_session_sim_and_drains_its_log() {
        let args = session_run_args("enabled", true);
        let log_file = args.log_file.clone().unwrap();

        let exit_code = run(&args).await;
        assert_eq!(exit_code, 0);

        let contents = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            contents.contains("session | session-script-ran"),
            "expected a session-prefixed log line, got:\n{contents}"
        );
    }

    #[tokio::test]
    async fn ut_run_with_zero_enabled_scripts_spawns_no_session_sim() {
        let args = session_run_args("disabled", false);
        let log_file = args.log_file.clone().unwrap();

        let exit_code = run(&args).await;
        assert_eq!(exit_code, 0);

        let contents = std::fs::read_to_string(&log_file).unwrap();
        assert!(
            !contents.contains("session |"),
            "no session source should appear in the log when no script is enabled, got:\n{contents}"
        );
    }
}
