use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crossterm::event::{KeyCode, KeyModifiers};

use crate::module::view::ModuleView;

/// Factory that produces a ready-to-use [`ModuleView`]. Obtains the log channel via
/// [`ModuleView::log`] after construction.
pub type ModuleViewFactory = Box<dyn FnOnce() -> Box<dyn ModuleView>>;

/// A module type's setup dialog — shown during creation (`:new`).
///
/// The concrete implementation lives inside the module-type directory and is
/// opaque to `App`.
pub trait SetupView {
    fn render(&mut self, area: Rect, buf: &mut Buffer);
    fn handle_events(&mut self, modifiers: KeyModifiers, code: KeyCode);
    fn focus_next(&mut self);
    fn focus_previous(&mut self);
    /// On confirm: return `(tab_name, factory)` or `None` if validation fails.
    fn confirm(&self) -> Option<(String, ModuleViewFactory)>;
}

/// Describes one module type available in the static registry.
#[allow(dead_code)]
pub struct ModuleTypeDescriptor {
    pub label: &'static str,
    /// Construct a fresh (empty) setup dialog for this module type.
    pub new_setup_view: fn() -> Box<dyn SetupView>,
}
