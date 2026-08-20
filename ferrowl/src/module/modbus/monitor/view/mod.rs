//! `ModbusMonitorModuleView` (UI-R-060/061): the monitor module's content view — a left panel
//! listing observed unit ids, and three right-hand sections scoped to the selected unit id: a
//! message table (MB-R-143), a memory layout grouped by table kind (MB-R-144), and a
//! resolved-registers table (MB-R-145), the last hidden entirely when no interpretation exists
//! for the selected unit id.

use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_codec::Kind;
use ferrowl_modbus::{Key, SlaveKey, UnitId};
use ferrowl_ui::EventResult;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::{LOG_SIZE, Level};
use crate::config::device::MonitorRegisterDef;
use crate::config::{ModuleSpec, MonitorDeviceConfig};
use crate::module::view::{
    CommandDescriptor, CommandFuture, CommandResult, CommandSpec, ModuleView, RefreshFuture,
    SharedLog, parse_command,
};

use super::ModbusMonitorModule;

/// Save `device` to `path`, mirroring `ModbusModuleView::save_device_to`'s pattern (stamps the
/// current `VERSION`, format from the path's extension).
#[allow(dead_code)] // consumed starting s8
fn save_device_to(device: &MonitorDeviceConfig, path: &str) -> CommandResult {
    use ferrowl_util::convert::{Converter, FileType};
    let Some(ty) = FileType::from_path(path) else {
        return CommandResult::Handled(Some((
            Level::Warning,
            format!("unknown format for '{path}' (use .toml or .json)"),
        )));
    };
    let mut device = device.clone();
    device.version = Some(crate::config::VERSION.to_string());
    match Converter::save(&device, path, ty) {
        Ok(()) => CommandResult::Handled(Some((
            Level::Info,
            format!("Saved device config to {path}"),
        ))),
        Err(e) => CommandResult::Handled(Some((Level::Error, format!("Save failed: {e:?}")))),
    }
}

/// The 4 register-table kinds a monitor's observed-value table groups its memory layout by
/// (MB-R-144), in display order.
#[allow(dead_code)] // consumed starting s8
const TABLE_KINDS: [Kind; 4] = [
    Kind::Coil,
    Kind::DiscreteInput,
    Kind::HoldingRegister,
    Kind::InputRegister,
];

// Forward-declared: real app-side construction lands in s8 of the modbus-bus-monitor plan (wiring the 3 construction call sites); already fully implemented and tested here.
#[allow(dead_code)]
pub struct ModbusMonitorModuleView {
    module: ModbusMonitorModule,
    spec: ModuleSpec,
    device: MonitorDeviceConfig,
    /// Every unit id observed so far, sorted, refreshed each tick (UI-R-060).
    unit_ids: Vec<UnitId>,
    /// Index into `unit_ids` of the left panel's current selection.
    selected: usize,
    /// MB-R-143 log lines for the selected unit id, re-derived each tick from `module.log()`.
    messages: Vec<String>,
    view_focused: bool,
}

#[allow(dead_code)] // forward-declared; see struct's note
impl ModbusMonitorModuleView {
    pub fn new(module: ModbusMonitorModule, spec: ModuleSpec, device: MonitorDeviceConfig) -> Self {
        Self {
            module,
            spec,
            device,
            unit_ids: Vec::new(),
            selected: 0,
            messages: Vec::new(),
            view_focused: false,
        }
    }

    fn selected_unit(&self) -> Option<UnitId> {
        self.unit_ids.get(self.selected).copied()
    }

    /// Interpretations defined for `unit` (MB-R-145), by name.
    fn interpretations_for(&self, unit: UnitId) -> Vec<(&String, &MonitorRegisterDef)> {
        self.module
            .interpretations()
            .iter()
            .filter(|(_, def)| def.slave_id == unit.0)
            .map(|(name, def)| (name, def))
            .collect()
    }

    /// Memory layout for `unit`, grouped by table kind (MB-R-144), non-empty kinds only.
    fn memory_rows(&self, unit: UnitId) -> Vec<(Kind, Vec<(u16, u16)>)> {
        let table = self.module.table();
        let table = table.read();
        TABLE_KINDS
            .iter()
            .filter_map(|kind| {
                let key = Key::new(SlaveKey {
                    slave_id: unit,
                    kind: kind.clone(),
                });
                let dump = table.dump(&key);
                if dump.is_empty() {
                    None
                } else {
                    Some((kind.clone(), dump))
                }
            })
            .collect()
    }
}

impl ferrowl_ui::traits::SetFocus for ModbusMonitorModuleView {
    fn set_focused(&mut self, focus: bool) {
        self.view_focused = focus;
    }
}

impl ferrowl_ui::traits::IsFocus for ModbusMonitorModuleView {
    fn is_focused(&self) -> bool {
        self.view_focused
    }
}

impl ModuleView for ModbusMonitorModuleView {
    fn name(&self) -> String {
        self.spec.name.clone()
    }

    fn is_overlay_active(&self) -> bool {
        false
    }

    fn render(&mut self, frame: &mut Frame, area: Rect) {
        use ratatui::layout::{Constraint, Layout};
        use ratatui::widgets::{Block, Borders, List, ListItem};

        let [left_area, right_area] =
            Layout::horizontal([Constraint::Length(12), Constraint::Min(1)]).areas(area);

        let buf = frame.buffer_mut();

        let items: Vec<ListItem> = self
            .unit_ids
            .iter()
            .enumerate()
            .map(|(idx, unit)| {
                let label = format!("{unit}");
                if idx == self.selected {
                    ListItem::new(format!("> {label}"))
                } else {
                    ListItem::new(format!("  {label}"))
                }
            })
            .collect();
        let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Units"));
        ratatui::widgets::Widget::render(list, left_area, buf);

        let selected = self.selected_unit();
        let has_interpretation = selected.is_some_and(|u| !self.interpretations_for(u).is_empty());

        let sections: Vec<Constraint> = if has_interpretation {
            vec![
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ]
        } else {
            vec![Constraint::Percentage(50), Constraint::Percentage(50)]
        };
        let section_areas = Layout::vertical(sections).split(right_area);

        // Message table (MB-R-143).
        let messages_block = Block::default()
            .borders(Borders::ALL)
            .title("Messages (MB-R-143)");
        let messages_inner = messages_block.inner(section_areas[0]);
        ratatui::widgets::Widget::render(messages_block, section_areas[0], buf);
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(self.messages.join("\n")),
            messages_inner,
            buf,
        );

        // Memory layout, grouped by table kind (MB-R-144).
        let memory_block = Block::default()
            .borders(Borders::ALL)
            .title("Memory layout (MB-R-144)");
        let memory_inner = memory_block.inner(section_areas[1]);
        ratatui::widgets::Widget::render(memory_block, section_areas[1], buf);
        let memory_text = if let Some(unit) = selected {
            self.memory_rows(unit)
                .into_iter()
                .map(|(kind, pairs)| {
                    let pairs_str = pairs
                        .iter()
                        .map(|(addr, word)| format!("{addr}={word}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("{kind:?}: {pairs_str}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(memory_text),
            memory_inner,
            buf,
        );

        // Resolved registers (MB-R-145) — omitted entirely when no interpretation exists for
        // the selected unit id (UI-R-061).
        if has_interpretation && let Some(unit) = selected {
            let resolved_block = Block::default()
                .borders(Borders::ALL)
                .title("Resolved registers (MB-R-145)");
            let resolved_inner = resolved_block.inner(section_areas[2]);
            ratatui::widgets::Widget::render(resolved_block, section_areas[2], buf);
            let table = self.module.table();
            let table = table.read();
            let text = self
                .interpretations_for(unit)
                .into_iter()
                .map(|(name, def)| {
                    let key = Key::new(SlaveKey {
                        slave_id: unit,
                        kind: def.kind.clone(),
                    });
                    let address = match def.address() {
                        ferrowl_codec::Address::Fixed(a) => a,
                        ferrowl_codec::Address::Virtual => 0,
                    };
                    let width = def.format().width();
                    match table.read_words(&key, address, width) {
                        Some(words) => {
                            format!("{name}: {words:?}")
                        }
                        None => format!("{name}: (not yet observed)"),
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            ratatui::widgets::Widget::render(
                ratatui::widgets::Paragraph::new(text),
                resolved_inner,
                buf,
            );
        }
    }

    fn render_overlay(&mut self, _frame: &mut Frame, _area: Rect) {}

    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        match code {
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                EventResult::Consumed
            }
            KeyCode::Down => {
                if self.selected + 1 < self.unit_ids.len() {
                    self.selected += 1;
                }
                EventResult::Consumed
            }
            _ => EventResult::Unhandled(modifiers, code),
        }
    }

    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a> {
        Box::pin(async move {
            {
                let table = self.module.table();
                let table = table.read();
                self.unit_ids = table.unit_ids();
            }
            if self.selected >= self.unit_ids.len() {
                self.selected = self.unit_ids.len().saturating_sub(1);
            }

            let lines = self.module.log().read().await.peek_n(LOG_SIZE);
            self.messages = match self.selected_unit() {
                Some(unit) => {
                    let prefix = format!("slave {unit} ");
                    lines
                        .into_iter()
                        .filter(|(_, _, s)| s.starts_with(&prefix))
                        .map(|(_, _, s)| s)
                        .collect()
                }
                None => Vec::new(),
            };
        })
    }

    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a> {
        let Some(parsed) = parse_command(&MONITOR_COMMAND_SPECS, cmd) else {
            return Box::pin(std::future::ready(CommandResult::Unhandled));
        };

        match parsed {
            ModbusMonitorCmd::Start => Box::pin(async move {
                let endpoint = self.spec.endpoint.to_string();
                match self
                    .module
                    .start(move |_s: String| async {}, move |_s: String| async {})
                    .await
                {
                    Ok(()) => CommandResult::Handled(Some((
                        Level::Info,
                        format!("Started monitor on {endpoint}"),
                    ))),
                    Err(e) => CommandResult::Handled(Some((
                        Level::Error,
                        format!("Start monitor failed: {e}"),
                    ))),
                }
            }),

            ModbusMonitorCmd::Stop => Box::pin(async move {
                match self.module.stop().await {
                    Ok(()) => CommandResult::Handled(Some((Level::Info, "Stopped monitor".into()))),
                    Err(e) => CommandResult::Handled(Some((
                        Level::Error,
                        format!("Stop monitor failed: {e}"),
                    ))),
                }
            }),

            ModbusMonitorCmd::Restart => Box::pin(async move {
                let endpoint = self.spec.endpoint.to_string();
                let _ = self.module.stop().await;
                match self
                    .module
                    .start(move |_s: String| async {}, move |_s: String| async {})
                    .await
                {
                    Ok(()) => CommandResult::Handled(Some((
                        Level::Info,
                        format!("Restarted monitor on {endpoint}"),
                    ))),
                    Err(e) => CommandResult::Handled(Some((
                        Level::Error,
                        format!("Restart monitor failed: {e}"),
                    ))),
                }
            }),

            ModbusMonitorCmd::Reload => Box::pin(async move {
                if self.spec.device.is_empty() {
                    return CommandResult::Handled(Some((
                        Level::Warning,
                        "No configuration file path configured. Reload aborted.".into(),
                    )));
                }
                let path = self.spec.device.clone();
                let device: MonitorDeviceConfig = match crate::config::load_monitor_device(&path) {
                    Ok(d) => d,
                    Err(e) => {
                        return CommandResult::Handled(Some((
                            Level::Error,
                            format!(":reload failed to load '{path}': {e}"),
                        )));
                    }
                };
                let _ = self.module.stop().await;
                let new_module = ModbusMonitorModule::new(&self.spec, &device);
                self.module = new_module;
                self.device = device;
                if let Err(e) = self
                    .module
                    .start(move |_s: String| async {}, move |_s: String| async {})
                    .await
                {
                    return CommandResult::Handled(Some((
                        Level::Error,
                        format!(":reload start error: {e}"),
                    )));
                }
                CommandResult::Handled(Some((Level::Info, format!(":reload done — '{path}'"))))
            }),

            ModbusMonitorCmd::Edit => {
                // Setup/:edit dialog wiring lands in s7 of the modbus-bus-monitor plan.
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }

            ModbusMonitorCmd::Add => {
                // Interpretation-add dialog wiring lands in s6 of the modbus-bus-monitor plan.
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }

            ModbusMonitorCmd::Compact => Box::pin(std::future::ready(CommandResult::Handled(None))),

            ModbusMonitorCmd::WriteDevice(rest) => {
                let path = rest.unwrap_or_else(|| self.spec.device.clone());
                if path.is_empty() {
                    return Box::pin(std::future::ready(CommandResult::Handled(Some((
                        Level::Warning,
                        "No configuration file path configured.".into(),
                    )))));
                }
                let result = save_device_to(&self.device, &path);
                Box::pin(std::future::ready(result))
            }

            ModbusMonitorCmd::Log(None) => Box::pin(std::future::ready(CommandResult::Unhandled)),

            ModbusMonitorCmd::Log(Some(file)) => {
                self.device.log_file = Some(file.clone());
                Box::pin(std::future::ready(CommandResult::Handled(Some((
                    Level::Info,
                    format!("Logging to files based on {file} (':wd' to persist)"),
                )))))
            }

            ModbusMonitorCmd::Order(_) => {
                Box::pin(std::future::ready(CommandResult::Handled(None)))
            }
        }
    }

    fn commands(&self) -> &[CommandDescriptor] {
        static DESCRIPTORS: std::sync::OnceLock<Vec<CommandDescriptor>> =
            std::sync::OnceLock::new();
        DESCRIPTORS.get_or_init(|| MONITOR_COMMAND_SPECS.iter().map(|s| s.descriptor).collect())
    }

    fn log(&self) -> SharedLog {
        self.module.log()
    }

    fn session_spec(&self) -> Option<serde_json::Value> {
        let mut v = serde_json::to_value(&self.spec).ok()?;
        v.as_object_mut()?.insert("type".into(), "modbus".into());
        Some(v)
    }
}

/// The parsed form of every command this view accepts; produced by [`parse_command`] over
/// [`MONITOR_COMMAND_SPECS`]. `:set`/`:script` are simply absent from the table — `parse_command`
/// already falls through to `CommandResult::Unhandled` for anything not listed, which is exactly
/// tui/api-contract.md's "both are unrecognized on this role rather than erroring".
#[allow(dead_code)] // consumed starting s8
enum ModbusMonitorCmd {
    Start,
    Stop,
    Restart,
    Reload,
    Edit,
    Add,
    Compact,
    WriteDevice(Option<String>),
    Log(Option<String>),
    Order(Option<String>),
}

/// Single source for this view's commands: aliases, help row, and parse target per entry.
#[allow(dead_code)] // consumed starting s8
static MONITOR_COMMAND_SPECS: [CommandSpec<ModbusMonitorCmd>; 10] = [
    CommandSpec {
        aliases: &["e", "edit"],
        descriptor: CommandDescriptor {
            name: ":e | :edit",
            description: "edit module setup",
        },
        build: |_| ModbusMonitorCmd::Edit,
    },
    CommandSpec {
        aliases: &["a", "add"],
        descriptor: CommandDescriptor {
            name: ":a | :add",
            description: "add register interpretation",
        },
        build: |_| ModbusMonitorCmd::Add,
    },
    CommandSpec {
        aliases: &["start"],
        descriptor: CommandDescriptor {
            name: ":start",
            description: "start module",
        },
        build: |_| ModbusMonitorCmd::Start,
    },
    CommandSpec {
        aliases: &["stop"],
        descriptor: CommandDescriptor {
            name: ":stop",
            description: "stop module",
        },
        build: |_| ModbusMonitorCmd::Stop,
    },
    CommandSpec {
        aliases: &["restart"],
        descriptor: CommandDescriptor {
            name: ":restart",
            description: "restart module",
        },
        build: |_| ModbusMonitorCmd::Restart,
    },
    CommandSpec {
        aliases: &["reload"],
        descriptor: CommandDescriptor {
            name: ":reload",
            description: "reload device config",
        },
        build: |_| ModbusMonitorCmd::Reload,
    },
    CommandSpec {
        aliases: &["compact"],
        descriptor: CommandDescriptor {
            name: ":compact",
            description: "toggle compact mode",
        },
        build: |_| ModbusMonitorCmd::Compact,
    },
    CommandSpec {
        aliases: &["wd", "write-device"],
        descriptor: CommandDescriptor {
            name: ":wd | :write-device [path]",
            description: "save device config",
        },
        build: |rest| ModbusMonitorCmd::WriteDevice(rest.map(str::to_string)),
    },
    CommandSpec {
        aliases: &["log"],
        descriptor: CommandDescriptor {
            name: ":log <file>",
            description: "set log file",
        },
        build: |rest| ModbusMonitorCmd::Log(rest.map(str::to_string)),
    },
    CommandSpec {
        aliases: &["order"],
        descriptor: CommandDescriptor {
            name: ":order [col] [asc|desc]",
            description: "sort table by column",
        },
        build: |rest| ModbusMonitorCmd::Order(rest.map(str::to_string)),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Endpoint, Role};
    use ferrowl_modbus::UnitId;

    fn spec() -> ModuleSpec {
        ModuleSpec {
            name: "mon1".to_string(),
            device: String::new(),
            role: Role::Monitor,
            endpoint: Endpoint::Rtu {
                path: "/dev/none".to_string(),
                baud_rate: 9600,
                parity: None,
                data_bits: None,
                stop_bits: None,
            },
        }
    }

    fn device() -> MonitorDeviceConfig {
        MonitorDeviceConfig::default()
    }

    fn view() -> ModbusMonitorModuleView {
        let module = ModbusMonitorModule::new(&spec(), &device());
        ModbusMonitorModuleView::new(module, spec(), device())
    }

    /// UI-R-060 — the left panel's `unit_ids` refresh from the observed table, live.
    #[tokio::test]
    async fn ut_refresh_populates_unit_ids_from_observed_table() {
        let mut v = view();
        {
            let table = v.module.table();
            let mut table = table.write();
            table.write_words(
                Key::new(SlaveKey {
                    slave_id: UnitId(3),
                    kind: Kind::HoldingRegister,
                }),
                0,
                &[1],
            );
        }
        v.refresh().await;
        assert_eq!(v.unit_ids, vec![UnitId(3)]);
    }

    /// UI-R-061 — the resolved-registers section is omitted entirely from the rendered buffer
    /// when no interpretation exists for the selected unit id, and reappears once one does.
    #[test]
    fn ut_resolved_registers_section_hidden_when_no_interpretation_for_selected_unit() {
        use ratatui::Terminal;
        use ratatui::backend::TestBackend;

        let mut v = view();
        v.unit_ids = vec![UnitId(3)];
        v.selected = 0;

        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        let contents =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut acc, cell| {
                    acc.push_str(cell.symbol());
                    acc
                });
        assert!(!contents.contains("Resolved registers"));

        v.module.add_interpretation(
            "power".to_string(),
            MonitorRegisterDef {
                slave_id: 3,
                kind: Kind::HoldingRegister,
                address: Some(0),
                is_virtual: false,
                value_type: crate::config::device::ValueType::U16,
                endian: Default::default(),
                word_order: Default::default(),
                resolution: 1.0,
                bitmask: None,
                length: 1,
                alignment: Default::default(),
                values: vec![],
                description: String::new(),
                default: None,
            },
        );

        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                v.render(frame, area);
            })
            .unwrap();
        let contents =
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .fold(String::new(), |mut acc, cell| {
                    acc.push_str(cell.symbol());
                    acc
                });
        assert!(contents.contains("Resolved registers"));
    }

    /// tui/api-contract.md's monitor command table — `:set`/`:script` are simply absent, so
    /// both fall through `parse_command` to `Unhandled` rather than erroring.
    #[tokio::test]
    async fn ut_set_and_script_commands_unrecognized() {
        let mut v = view();
        assert!(matches!(
            v.handle_command("set foo 1").await,
            CommandResult::Unhandled
        ));
        assert!(matches!(
            v.handle_command("script").await,
            CommandResult::Unhandled
        ));
    }

    /// Every entry in `MONITOR_COMMAND_SPECS` routes through `handle_command` without panicking.
    #[tokio::test]
    async fn ut_every_monitor_command_spec_has_matching_handler() {
        let mut v = view();
        for spec in &MONITOR_COMMAND_SPECS {
            for alias in spec.aliases {
                let _ = v.handle_command(alias).await;
            }
        }
    }

    /// `commands()`/`log()` delegate to the command table and the module's own log ring.
    #[test]
    fn ut_monitor_view_commands_and_log_delegate() {
        let v = view();
        assert_eq!(v.commands().len(), MONITOR_COMMAND_SPECS.len());
        let _log: SharedLog = v.log();
    }
}
