use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::LogRing;

/// Generic log channel shared between a [`ModuleView`] and the owning [`Tab`].
pub type SharedLog = std::sync::Arc<tokio::sync::RwLock<LogRing>>;

/// Result returned by [`ModuleView::handle_command`].
pub enum CommandResult {
    /// Command was handled; optional message to append to the tab log.
    Handled(Option<String>),
    /// Command is not known to this module.
    Unhandled,
}

/// One entry in a module's command help list.
pub struct CommandDescriptor {
    pub name: &'static str,
    pub description: &'static str,
}

/// Object-safe async return type for [`ModuleView::handle_command`].
pub type CommandFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = CommandResult> + 'a>>;

pub type RefreshFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>;

/// The trait every module content view must implement.
///
/// `Tab` and `App` interact with a module exclusively through this interface.
/// No module-type-specific types are visible outside the module's own directory.
pub trait ModuleView {
    // Name of the module instance
    fn name(&self) -> String;

    /// Render the module content area (everything except the log pane and tab bar).
    /// `focused` is true when the view's content pane (not log) has keyboard focus.
    fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool);

    /// Handle a terminal key event. Returns `Consumed` or `Unhandled`.
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult;

    /// Pull a fresh snapshot from internal backends and update render state.
    /// Called once per UI tick before [`render`].
    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a>;

    /// Execute a module command string asynchronously.
    ///
    /// Standard commands dispatched by App: `"start"`, `"stop"`, `"restart"`,
    /// `"reload"`, `"edit"`, `"add"`, `"compact"`, `"wd [path]"`, `"log <file>"`,
    /// `"set <reg> <val>"`.
    fn handle_command<'a>(&'a mut self, cmd: &'a str) -> CommandFuture<'a>;

    /// Module-specific commands shown in the help popup.
    fn commands(&self) -> &[CommandDescriptor];

    /// The log channel written by this view's backend.
    fn log(&self) -> SharedLog;

    /// Serialize this module's config for session persistence, or `None` if unsupported.
    /// The returned value should include a `"type"` field so the loader can dispatch to
    /// the right deserializer (e.g. `"modbus"`, `"ocpp"`).
    fn session_spec(&self) -> Option<serde_json::Value> {
        None
    }
}
