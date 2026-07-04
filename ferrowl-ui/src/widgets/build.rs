//! Shared widget constructors invoked from outside this crate (currently the modbus dialog
//! and OCPP overlay build code), hoisted here because their shape is identical across callers.
//!
//! Each builder here is invoked with a complete, statically-known configuration (all widget
//! and state builder fields carry defaults), so construction is infallible. The single `expect`
//! per constructor documents that invariant instead of scattering `unwrap()` calls through
//! caller code.

use crate::state::{ButtonState, ButtonStateBuilder};
use crate::style::ButtonStyle;
use crate::widgets::{Button, ButtonBuilder, Widget};
use ratatui::layout::{HorizontalAlignment, Margin};

/// An unfocused, center-aligned button labelled `label`, styled with `style` and given
/// `horizontal` outer margin.
pub fn button(label: &str, style: ButtonStyle, horizontal: u16) -> Widget<ButtonState, Button> {
    Widget {
        state: ButtonStateBuilder::default()
            .focused(false)
            .label(label.to_string())
            .disabled(false)
            .build()
            .expect("static button state"),
        widget: ButtonBuilder::default()
            .border_margin(Margin::new(1, 0))
            .margin(Margin {
                vertical: 0,
                horizontal,
            })
            .style(style)
            .horizontal_alignment(HorizontalAlignment::Center)
            .build()
            .expect("static button config"),
    }
}
