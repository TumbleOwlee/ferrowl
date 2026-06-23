//! Proc macros for keyboard focus handling in ferrowl UI views.
//!
//! Use [`macro@focusable`] to inject a `focus` state field into a view
//! struct, then `#[derive(Focus)]` with `#[focus]` field attributes to
//! generate focus cycling and event dispatch for its widgets.

extern crate proc_macro;
use convert_case::{Case, Casing};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{
    Expr, Field, Fields, Ident, Meta, MetaNameValue, Token, Type, Visibility,
    punctuated::Punctuated,
};

struct Definition {
    widget_name: Ident,
    enum_field: Ident,
    when: Option<Expr>,
}

/// Derives focus cycling and event dispatch for a view struct.
///
/// For every field marked `#[focus]` (optionally gated with
/// `#[focus(when = condition)]`), the macro generates:
///
/// - a `<StructName>Focus` enum with one variant per focusable field,
/// - `focus_previous()`/`focus_next()` methods that cycle focus through the
///   marked fields (skipping those whose `when` condition is false) and call
///   `SetFocus::set_focused` on the widgets,
/// - a `HandleEvents` impl forwarding key events to the focused widget.
///
/// The struct must have a `focus: <StructName>Focus` field — usually
/// injected with [`macro@focusable`].
#[proc_macro_derive(Focus, attributes(focus))]
pub fn derive_focus(item: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(item as syn::DeriveInput);
    let identifier = &input.ident;
    let (impl_generic, ty_generic, where_clause) = &input.generics.split_for_impl();

    match &mut input.data {
        syn::Data::Struct(s) => {
            let mut definitions = vec![];

            // Iterate over fields and look for #[focus] attributes
            for field in s.fields.iter() {
                let mut found = false;
                let mut when: Option<Expr> = None;

                for attr in field.attrs.iter() {
                    if attr.path().is_ident("focus") {
                        found = true;

                        // No arguments, just #[focus]
                        if let Meta::Path(_) = attr.meta {
                            continue;
                        }

                        // Parse arguments for #[focus(when = some_condition)]
                        if let Ok(args) =
                            attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                        {
                            for arg in args {
                                if let Meta::NameValue(MetaNameValue { path, value, .. }) = arg {
                                    if path.is_ident("when") {
                                        when = Some(value);
                                    }
                                } else {
                                    return syn::Error::new_spanned(&arg, "Invalid argument for #[focus] attribute, expected key-value pairs like #[focus(when = some_condition)]")
                                        .to_compile_error()
                                        .into();
                                }
                            }
                        } else {
                            // Invalid syntax for #[focus] attribute
                            return syn::Error::new_spanned(attr, "Invalid syntax for #[focus] attribute, expected #[focus(when = some_condition)]")
                                        .to_compile_error()
                                        .into();
                        }
                    }
                }

                // If #[focus] attribute is found, add to definitions
                if found {
                    if let Some(ident) = &field.ident {
                        let enum_field = format!("{}", ident).to_case(Case::Pascal);
                        definitions.push(Definition {
                            widget_name: ident.clone(),
                            enum_field: Ident::new(&enum_field, Span::call_site()),
                            when,
                        });
                    } else {
                        // Unnamed fields are not supported for focus switching
                        return syn::Error::new_spanned(
                            field,
                            "FocusSwitch only works on named fields with ident.",
                        )
                        .to_compile_error()
                        .into();
                    }
                }
            }

            // Number of focusable fields
            let def_len = definitions.len();

            // Generate enum name based on struct name
            let enum_name = identifier.to_string() + "Focus";
            let enum_name = Ident::new(&enum_name, Span::call_site());

            // Create static array for indexing
            let enum_fields = definitions.iter().map(|i| &i.enum_field);
            let impl_array = quote! {
                // Array for static indexing
                let focuses = [#(#enum_name::#enum_fields),*];
            };

            // Generate code for disabling current focus
            let mut impl_disable = quote! {};
            for def in definitions.iter() {
                let name = &def.widget_name;
                let enum_field = &def.enum_field;
                impl_disable.extend(quote! {
                    #enum_name::#enum_field => {ferrowl_ui::traits::SetFocus::set_focused(&mut self.#name, false);}
                });
            }
            let impl_disable = quote! {
                match self.focus {
                    #impl_disable
                    _ => {unreachable!("Invalid focus state");},
                }
            };

            // Generate code for enabling new focus
            let mut impl_enable = quote! {};
            for def in definitions.iter() {
                let name = &def.widget_name;
                let enum_field = &def.enum_field;
                let when = if let Some(when) = &def.when {
                    quote! {
                        && #when
                    }
                } else {
                    quote! {}
                };

                impl_enable.extend(quote! {
                    if current_focus == #enum_name::#enum_field #when {
                        ferrowl_ui::traits::SetFocus::set_focused(&mut self.#name, true);
                        self.focus = #enum_name::#enum_field;
                        break;
                    }
                });
            }

            // Common code for both previous and next focus switching
            let impl_general = quote! {
                #impl_array

                #impl_disable

                // Get index of current focus
                let index = focuses.iter().position(|f| *f == self.focus).unwrap();
            };

            // Forward and reverse traversal differ only by the per-step `delta` (forward = +1,
            // reverse = +(len-1), i.e. -1 mod len).
            let focus_loop = |delta: proc_macro2::TokenStream| {
                quote! {
                    #impl_general

                    let mut current_index = (index + #delta) % #def_len;

                    loop {
                        let current_focus = focuses[current_index];

                        #impl_enable

                        if current_index == index {
                            break;
                        }

                        // Iterate
                        current_index = (current_index + #delta) % #def_len;
                    }
                }
            };
            let impl_previous = focus_loop(quote! { (#def_len - 1) });
            let impl_next = focus_loop(quote! { 1 });

            // Generate implementation for focus switching methods.
            let focus_def = quote! {
                impl #impl_generic #identifier #ty_generic #where_clause {
                    // `% #def_len` collapses to `% 1` for single-field views; that is
                    // correct (it always yields the one field) but trips `modulo_one`.
                    #[allow(clippy::modulo_one)]
                    pub fn focus_previous(&mut self) {
                        #impl_previous
                    }
                    #[allow(clippy::modulo_one)]
                    pub fn focus_next(&mut self) {
                        #impl_next
                    }
                }
            };

            // `set_focused`/`is_focused` for the whole view (a `#[focus]`-bearing struct is itself a
            // focusable node, so it composes with parent views). Enabling restores the remembered
            // pane if its `#[focus(when=…)]` guard still holds, else the first eligible pane;
            // disabling unfocuses every child and keeps the remembered pane.
            let mut impl_clear_all = quote! {};
            let mut impl_eligibility = quote! {};
            let mut impl_candidates = quote! {};
            let mut impl_focus_one = quote! {};
            for def in definitions.iter() {
                let name = &def.widget_name;
                let enum_field = &def.enum_field;
                let when = match &def.when {
                    Some(when) => quote! { #when },
                    None => quote! { true },
                };
                impl_clear_all.extend(quote! {
                    ferrowl_ui::traits::SetFocus::set_focused(&mut self.#name, false);
                });
                impl_eligibility.extend(quote! {
                    #enum_name::#enum_field => #when,
                });
                impl_candidates.extend(quote! {
                    (#enum_name::#enum_field, #when),
                });
                impl_focus_one.extend(quote! {
                    #enum_name::#enum_field => ferrowl_ui::traits::SetFocus::set_focused(&mut self.#name, true),
                });
            }
            let set_focus_def = quote! {
                impl #impl_generic ferrowl_ui::traits::IsFocus for #identifier #ty_generic #where_clause {
                    fn is_focused(&self) -> bool {
                        self.view_focused
                    }
                }
                impl #impl_generic ferrowl_ui::traits::SetFocus for #identifier #ty_generic #where_clause {
                    fn set_focused(&mut self, focus: bool) {
                        self.view_focused = focus;
                        #impl_clear_all
                        if !focus {
                            return;
                        }
                        let remembered_ok = match self.focus {
                            #impl_eligibility
                        };
                        if !remembered_ok {
                            let candidates = [ #impl_candidates ];
                            if let Some(&(f, _)) = candidates.iter().find(|&&(_, ok)| ok) {
                                self.focus = f;
                            }
                        }
                        match self.focus {
                            #impl_focus_one
                        }
                    }
                }
            };

            // Generate Enum for focus states
            let enum_fields = definitions.iter().map(|i| &i.enum_field);
            let enum_def = quote! {
                #[derive(Debug, Clone, Copy, PartialEq)]
                pub enum #enum_name {
                    #(#enum_fields),*
                }
            };

            // Implementation of HandleEvents
            let mut impl_handle_events = quote! {};
            for def in definitions.iter() {
                let from = &def.widget_name;
                let from_enum = &def.enum_field;
                impl_handle_events.extend(quote! {
                    #enum_name::#from_enum => ferrowl_ui::traits::HandleEvents::handle_events(&mut self.#from, modifiers, code),
                });
            }

            let handle_def = quote! {
                impl #impl_generic ferrowl_ui::traits::HandleEvents for #identifier #ty_generic #where_clause {
                    fn handle_events(&mut self, modifiers: crossterm::event::KeyModifiers, code: crossterm::event::KeyCode) -> ferrowl_ui::EventResult {
                        match self.focus {
                            #impl_handle_events
                            _ => unreachable!("Invalid focus state"),
                        }
                    }
                }
            };

            // Output genereted code
            TokenStream::from(quote! {
                #enum_def
                #focus_def
                #set_focus_def
                #handle_def
            })
        }
        _ => unimplemented!("State not implemented for type"),
    }
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
    expand_table_entry(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_table_entry(input: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = input.ident.clone();
    let vis = input.vis.clone();
    let generics = input.generics.clone();
    let (impl_generic, ty_generic, where_clause) = generics.split_for_impl();

    // Struct-level attributes.
    let mut header_ident: Option<Ident> = None;
    let mut styles_path: Option<syn::Path> = None;
    let mut height: u16 = 1;

    for attr in &input.attrs {
        if attr.path().is_ident("table_entry") {
            attr.parse_nested_meta(|m| {
                if m.path.is_ident("header") {
                    header_ident = Some(m.value()?.parse()?);
                } else if m.path.is_ident("styles") {
                    styles_path = Some(m.value()?.parse()?);
                } else {
                    return Err(
                        m.error("unknown `table_entry` key (expected `header` or `styles`)")
                    );
                }
                Ok(())
            })?;
        } else if attr.path().is_ident("row") {
            attr.parse_nested_meta(|m| {
                if m.path.is_ident("height") {
                    height = m.value()?.parse::<syn::LitInt>()?.base10_parse()?;
                } else {
                    return Err(m.error("unknown `row` key (expected `height`)"));
                }
                Ok(())
            })?;
        }
    }

    // Column fields, in declaration order.
    let fields = match &input.data {
        syn::Data::Struct(s) => match &s.fields {
            Fields::Named(n) => &n.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &ident,
                    "TableEntry requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &ident,
                "TableEntry can only be derived for structs",
            ));
        }
    };

    let mut col_fields: Vec<Ident> = vec![];
    let mut col_names: Vec<syn::LitStr> = vec![];
    let mut col_mins: Vec<syn::LitInt> = vec![];
    let mut col_maxs: Vec<syn::LitInt> = vec![];

    for field in fields {
        for attr in &field.attrs {
            if !attr.path().is_ident("column") {
                continue;
            }
            let mut name: Option<syn::LitStr> = None;
            let mut min: Option<syn::LitInt> = None;
            let mut max: Option<syn::LitInt> = None;
            attr.parse_nested_meta(|m| {
                if m.path.is_ident("name") {
                    name = Some(m.value()?.parse()?);
                } else if m.path.is_ident("min") {
                    min = Some(m.value()?.parse()?);
                } else if m.path.is_ident("max") {
                    max = Some(m.value()?.parse()?);
                } else {
                    return Err(m.error("unknown `column` key (expected `name`, `min`, `max`)"));
                }
                Ok(())
            })?;
            let err = |msg: &str| syn::Error::new_spanned(attr, msg);
            col_names.push(name.ok_or_else(|| err("`column` requires `name`"))?);
            col_mins.push(min.ok_or_else(|| err("`column` requires `min`"))?);
            col_maxs.push(max.ok_or_else(|| err("`column` requires `max`"))?);
            col_fields.push(field.ident.clone().unwrap());
        }
    }

    if col_fields.is_empty() {
        return Err(syn::Error::new_spanned(
            &ident,
            "TableEntry needs at least one `#[column(name = …, min = …, max = …)]` field",
        ));
    }
    let n = col_fields.len();

    let header_ident =
        header_ident.unwrap_or_else(|| Ident::new(&format!("{}Header", ident), ident.span()));

    let cell_styles_impl = styles_path.map(|path| {
        quote! {
            fn cell_styles(&self) -> [::core::option::Option<ratatui::style::Style>; #n] {
                #path(self)
            }
        }
    });

    Ok(quote! {
        impl #impl_generic ferrowl_ui::widgets::TableEntry<#n> for #ident #ty_generic #where_clause {
            fn values(&self) -> [::std::string::String; #n] {
                [ #( ::std::string::ToString::to_string(&self.#col_fields) ),* ]
            }
            fn height(&self) -> u16 {
                #height
            }
            #cell_styles_impl
        }

        #[derive(Clone, Copy, Debug, Default)]
        #vis struct #header_ident;

        impl ferrowl_ui::widgets::Header<#n> for #header_ident {
            fn header() -> [::std::string::String; #n] {
                [ #( #col_names.into() ),* ]
            }
            fn widths() -> [ferrowl_ui::widgets::Width; #n] {
                [ #( ferrowl_ui::widgets::Width { min: #col_mins, max: #col_maxs } ),* ]
            }
        }
    })
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
    expand_overlay(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_overlay(input: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = input.ident.clone();
    let generics = input.generics.clone();
    let (impl_generic, ty_generic, where_clause) = generics.split_for_impl();

    let data = match &input.data {
        syn::Data::Enum(e) => e,
        _ => {
            return Err(syn::Error::new_spanned(
                &ident,
                "Overlay can only be derived for enums",
            ));
        }
    };

    let mut none_variant: Option<Ident> = None;
    let mut arms: Vec<proc_macro2::TokenStream> = vec![];

    for v in &data.variants {
        let vname = &v.ident;
        let mut is_none = false;
        let mut esc_close = false;
        let mut focus_cycle = false;

        for attr in &v.attrs {
            if !attr.path().is_ident("overlay") {
                continue;
            }
            attr.parse_nested_meta(|m| {
                if m.path.is_ident("none") {
                    is_none = true;
                } else if m.path.is_ident("esc_close") {
                    esc_close = true;
                } else if m.path.is_ident("focus_cycle") {
                    focus_cycle = true;
                } else {
                    return Err(m.error(
                        "unknown `overlay` key (expected `none`, `esc_close`, `focus_cycle`)",
                    ));
                }
                Ok(())
            })?;
        }

        if is_none {
            if !matches!(v.fields, Fields::Unit) {
                return Err(syn::Error::new_spanned(
                    vname,
                    "`#[overlay(none)]` variant must be a unit variant",
                ));
            }
            if none_variant.is_some() {
                return Err(syn::Error::new_spanned(
                    vname,
                    "only one `#[overlay(none)]` variant is allowed",
                ));
            }
            none_variant = Some(vname.clone());
            arms.push(quote! {
                #ident::#vname => ferrowl_ui::traits::OverlayRoute::Unhandled,
            });
            continue;
        }

        let single_field = matches!(&v.fields, Fields::Unnamed(f) if f.unnamed.len() == 1);
        if (esc_close || focus_cycle) && !single_field {
            return Err(syn::Error::new_spanned(
                vname,
                "an `esc_close`/`focus_cycle` overlay variant must hold exactly one field",
            ));
        }

        if !esc_close && !focus_cycle {
            arms.push(quote! {
                #ident::#vname(..) => ferrowl_ui::traits::OverlayRoute::Unhandled,
            });
            continue;
        }

        let mut key_arms = quote! {};
        if focus_cycle {
            key_arms.extend(quote! {
                (crossterm::event::KeyModifiers::NONE, crossterm::event::KeyCode::Tab) => {
                    ferrowl_ui::traits::OverlayKeys::focus_cycle(inner, true);
                    ferrowl_ui::traits::OverlayRoute::Cycled
                }
                (
                    crossterm::event::KeyModifiers::NONE | crossterm::event::KeyModifiers::SHIFT,
                    crossterm::event::KeyCode::BackTab,
                ) => {
                    ferrowl_ui::traits::OverlayKeys::focus_cycle(inner, false);
                    ferrowl_ui::traits::OverlayRoute::Cycled
                }
            });
        }
        if esc_close {
            key_arms.extend(quote! {
                (crossterm::event::KeyModifiers::NONE, crossterm::event::KeyCode::Esc) => {
                    ferrowl_ui::traits::OverlayRoute::Closed
                }
            });
        }

        let binding = if focus_cycle {
            quote! { inner }
        } else {
            quote! { _ }
        };

        arms.push(quote! {
            #ident::#vname(#binding) => match (modifiers, code) {
                #key_arms
                _ => ferrowl_ui::traits::OverlayRoute::Unhandled,
            },
        });
    }

    let none_variant = none_variant.ok_or_else(|| {
        syn::Error::new_spanned(
            &ident,
            "Overlay enum needs a `#[overlay(none)]` unit variant",
        )
    })?;

    Ok(quote! {
        impl #impl_generic #ident #ty_generic #where_clause {
            /// True unless this overlay is in its `None` (inactive) state.
            pub fn is_active(&self) -> bool {
                !matches!(self, #ident::#none_variant)
            }
            /// Reset the overlay to its `None` state.
            pub fn close(&mut self) {
                *self = #ident::#none_variant;
            }
            /// Take the current overlay, leaving `None` in its place.
            pub fn take(&mut self) -> #ident #ty_generic {
                ::core::mem::replace(self, #ident::#none_variant)
            }
            /// Route the common overlay keys: `Esc` closes `esc_close` variants,
            /// `Tab`/`BackTab` cycle focus on `focus_cycle` variants (via
            /// `OverlayKeys`). Other keys return `Unhandled`.
            pub fn route_keys(
                &mut self,
                modifiers: crossterm::event::KeyModifiers,
                code: crossterm::event::KeyCode,
            ) -> ferrowl_ui::traits::OverlayRoute {
                let outcome = match self {
                    #(#arms)*
                };
                if matches!(outcome, ferrowl_ui::traits::OverlayRoute::Closed) {
                    *self = #ident::#none_variant;
                }
                outcome
            }
        }
    })
}

/// Attribute that appends the private state `#[derive(Focus)]` needs: a
/// `focus: <StructName>Focus` field (which pane is focused) and a
/// `view_focused: bool` field (whether the whole view is focused). Must appear
/// *above* the derive so the fields exist when the derive runs.
#[proc_macro_attribute]
pub fn focusable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(item as syn::DeriveInput);

    // Structs that also `#[derive(Builder)]` get `#[builder(default)]` on the injected
    // `view_focused` flag so callers needn't set it (it defaults to `false`); the `focus` field is
    // still set explicitly by those builders (its enum has no `Default`).
    let uses_builder = input.attrs.iter().any(|attr| {
        attr.path().is_ident("derive")
            && attr
                .parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)
                .map(|paths| paths.iter().any(|p| p.is_ident("Builder")))
                .unwrap_or(false)
    });

    match &mut input.data {
        syn::Data::Struct(s) => {
            let ident = input.ident.to_string().to_case(Case::Pascal) + "Focus";
            let focus_field = Field {
                attrs: Vec::new(),
                mutability: syn::FieldMutability::None,
                vis: Visibility::Inherited,
                ident: Some(Ident::new("focus", Span::call_site())),
                colon_token: Some(Default::default()),
                ty: syn::parse_str::<Type>(&ident).unwrap(),
            };
            let view_focused_attrs = if uses_builder {
                vec![syn::parse_quote!(#[builder(default)])]
            } else {
                Vec::new()
            };
            let view_focused_field = Field {
                attrs: view_focused_attrs,
                mutability: syn::FieldMutability::None,
                vis: Visibility::Inherited,
                ident: Some(Ident::new("view_focused", Span::call_site())),
                colon_token: Some(Default::default()),
                ty: syn::parse_str::<Type>("bool").unwrap(),
            };

            match &mut s.fields {
                Fields::Named(named) => {
                    named.named.push(focus_field);
                    named.named.push(view_focused_field);
                }
                _ => {
                    unreachable!("FocusSwitch only works on named fields.");
                }
            }

            TokenStream::from(quote! {
                #input
            })
        }
        _ => unimplemented!("State not implemented for type"),
    }
}
