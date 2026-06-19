use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ratatui::Frame;
use ratatui::layout::Rect;

use crate::app::LogRing;
use crate::dialog::EditedRegister;

/// Generic log channel shared between a [`ModuleView`] and the owning [`Tab`].
/// The view backend writes to it; the owning `Tab` reads it to populate the log pane.
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

/// Pending async work produced by a view after an internal dialog confirms.
/// App receives this after each key event and performs the async side-effects.
pub enum PendingViewAction {
    /// Confirmed edit of an existing register.
    EditRegister(EditedRegister),
    /// Confirmed addition of a new register.
    AddRegister(EditedRegister),
    /// User confirmed delete of the register with this name.
    DeleteRegister(String),
}

/// The trait every module content view must implement.
///
/// `Tab` and `App` interact with a module exclusively through this interface —
/// no module-type-specific types are visible outside the module's own directory.
pub trait ModuleView {
    /// Render the module content area (everything except the log pane and tab bar).
    /// `focused` is true when the view's content pane (not log) has keyboard focus.
    fn render(&mut self, frame: &mut Frame, area: Rect, focused: bool);

    /// Handle a terminal key event. Returns `Consumed` or `Unhandled`.
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult;

    /// Pull a fresh snapshot from internal backends and update render state.
    /// Called once per UI tick before [`render`].
    fn refresh(&mut self);

    /// Execute a module-specific command string (the part after `:`).
    /// Common app-level commands are handled by `App` before this is called.
    fn handle_command(&mut self, cmd: &str) -> CommandResult;

    /// Module-specific commands shown in the help popup.
    fn commands(&self) -> &[CommandDescriptor];

    /// Whether the underlying network instance is currently active/connected.
    fn is_active(&self) -> bool;

    /// The log channel written by this view's backend. `Tab` holds a clone of this
    /// handle so the log pane can be populated independently of the module type.
    fn log(&self) -> SharedLog;

    /// Take any pending async action produced by a completed internal dialog.
    /// App calls this after each key event; default returns `None`.
    fn take_pending(&mut self) -> Option<PendingViewAction> {
        None
    }

    /// Downcast support — required for temporary Modbus-specific access during migration.
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
