//! Proc macros for ferrowl UI views.
//!
//! - [`macro@focusable`] + `#[derive(Focus)]` — keyboard focus state, cycling, and event dispatch.
//! - `#[derive(TableEntry)]` — table row + companion `Header` ZST from `#[column(…)]` fields.
//! - `#[derive(Overlay)]` — structural helpers + common-key routing for overlay enums.
//!
//! Each derive's logic lives in its own module; the entry points below stay in the crate root
//! because proc-macro functions must be defined there.

extern crate proc_macro;

mod focus;
mod overlay;
mod table_entry;

use proc_macro::TokenStream;

/// Derives focus cycling and event dispatch for a view struct.
///
/// For every field marked `#[focus]` (optionally gated with
/// `#[focus(when = condition)]`), the macro generates:
///
/// - a `<StructName>Focus` enum with one variant per focusable field,
/// - `focus_previous()`/`focus_next()` methods that cycle focus through the
///   marked fields (skipping those whose `when` condition is false) and call
///   `SetFocus::set_focused` on the widgets,
/// - whole-view `SetFocus`/`IsFocus` impls (the view is itself a focusable node),
/// - a `HandleEvents` impl forwarding key events to the focused widget.
///
/// The struct must have a `focus: <StructName>Focus` field — usually
/// injected with [`macro@focusable`].
#[proc_macro_derive(Focus, attributes(focus))]
pub fn derive_focus(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);
    focus::expand_focus(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Attribute that appends the private state `#[derive(Focus)]` needs: a
/// `focus: <StructName>Focus` field (which pane is focused) and a
/// `view_focused: bool` field (whether the whole view is focused). Must appear
/// *above* the derive so the fields exist when the derive runs.
#[proc_macro_attribute]
pub fn focusable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);
    focus::expand_focusable(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derives [`TableEntry`] for a row struct and a companion `Header` ZST.
///
/// Each field tagged `#[column(name = "…", min = N, max = M)]` becomes one
/// column, in declaration order; untagged fields are ignored. The macro
/// generates:
///
/// - `impl ferrowl_ui::widgets::TableEntry<N>` whose `values()` are the column
///   fields stringified via `ToString`, and `height()` from an optional
///   struct-level `#[row(height = N)]` (default `1`),
/// - a unit struct `<StructName>Header` (override with
///   `#[table_entry(header = Name)]`) and its `impl Header<N>` built from the
///   same `name`/`min`/`max` attributes.
///
/// Status-colored rows opt into custom cell styling with
/// `#[table_entry(styles = path::to_fn)]`, where the function has signature
/// `fn(&Self) -> [Option<ratatui::style::Style>; N]`.
#[proc_macro_derive(TableEntry, attributes(column, row, table_entry))]
pub fn derive_table_entry(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);
    table_entry::expand_table_entry(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

/// Derives structural helpers and common-key routing for an overlay enum.
///
/// The enum models a set of mutually-exclusive overlays plus an inactive state.
/// Exactly one unit variant must be tagged `#[overlay(none)]`; the rest each
/// hold a single payload. The macro generates inherent methods:
///
/// - `is_active()` — true unless `None`,
/// - `close()` — reset to `None`,
/// - `take()` — take the overlay, leaving `None`,
/// - `route_keys(modifiers, code)` — for variants tagged `#[overlay(esc_close)]`
///   and/or `#[overlay(focus_cycle)]`, handle `Esc` (close) and `Tab`/`BackTab`
///   (via the [`OverlayKeys`](ferrowl_ui::traits::OverlayKeys) trait), returning
///   an [`OverlayRoute`](ferrowl_ui::traits::OverlayRoute). Any other key — and
///   any untagged variant — returns `Unhandled` so the view's own
///   `Enter`/custom handling still runs.
#[proc_macro_derive(Overlay, attributes(overlay))]
pub fn derive_overlay(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);
    overlay::expand_overlay(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
