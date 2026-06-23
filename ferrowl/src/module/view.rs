use crossterm::event::{KeyCode, KeyModifiers};
use ferrowl_ui::EventResult;
use ferrowl_ui::traits::{HandleEvents, IsFocus, SetFocus};
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
///
/// A module view is a focusable node ([`SetFocus`] + [`IsFocus`]): the owning [`Tab`] toggles its
/// whole-view focus, and the view reads [`IsFocus::is_focused`] for focus-dependent rendering (e.g.
/// message-log autoscroll). Concrete views get these from `#[derive(Focus)]`.
pub trait ModuleView: SetFocus + IsFocus {
    // Name of the module instance
    fn name(&self) -> String;

    /// Render the module content area (everything except the log pane and tab bar).
    /// Focus-dependent rendering reads [`IsFocus::is_focused`] on `self`.
    fn render(&mut self, frame: &mut Frame, area: Rect);

    /// Handle a terminal key event. Returns `Consumed` or `Unhandled`.
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult;

    /// Pull a fresh snapshot from internal backends and update render state.
    /// Called once per UI tick before [`render`].
    fn refresh<'a>(&'a mut self) -> RefreshFuture<'a>;

    /// Status if dialog is shown
    fn is_overlay_active(&self) -> bool;

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

    /// Take a view that should replace this one in its tab, if the view requested one (e.g. the
    /// OCPP role was switched in the edit dialog, turning a client view into a server view).
    /// Polled by `App` once per tick after [`refresh`]. Default: never replaced.
    fn take_replacement(&mut self) -> Option<Box<dyn ModuleView>> {
        None
    }
}

// Forwarding impls so a boxed module view is itself a focusable, event-handling node — lets the
// owning `Tab` carry it as a `#[focus]` field under `#[derive(Focus)]`.
impl SetFocus for Box<dyn ModuleView> {
    fn set_focused(&mut self, focus: bool) {
        (**self).set_focused(focus);
    }
}

impl IsFocus for Box<dyn ModuleView> {
    fn is_focused(&self) -> bool {
        (**self).is_focused()
    }
}

impl HandleEvents for Box<dyn ModuleView> {
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode) -> EventResult {
        (**self).handle_events(modifiers, code)
    }
}
