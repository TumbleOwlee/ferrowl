//! Per-widget style bundles, defaulting to [`COLOR_SCHEME`](crate::COLOR_SCHEME).

mod button;
mod input_field;
mod scrolling_tabs;
mod selection;
mod table;
mod text;

pub use button::*;
pub use input_field::*;
pub use scrolling_tabs::*;
pub use selection::*;
pub use table::*;
pub use text::*;
