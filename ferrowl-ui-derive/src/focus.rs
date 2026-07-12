//! `#[derive(Focus)]` and the `#[focusable]` attribute.

use convert_case::{Case, Casing};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{
    Expr, Field, Fields, Ident, Meta, MetaNameValue, Token, Type, Visibility,
    punctuated::Punctuated,
};

/// One focusable field, as gathered from a `#[focus]`/`#[focus(when = …)]` attribute.
struct Definition {
    widget_name: Ident,
    enum_field: Ident,
    when: Option<Expr>,
}

/// Collect the `#[focus]`-tagged fields of a struct in declaration order.
fn collect_definitions(fields: &Fields) -> syn::Result<Vec<Definition>> {
    let mut definitions = vec![];

    for field in fields.iter() {
        let mut found = false;
        let mut when: Option<Expr> = None;

        for attr in field.attrs.iter() {
            if !attr.path().is_ident("focus") {
                continue;
            }
            found = true;

            // No arguments, just `#[focus]`.
            if let Meta::Path(_) = attr.meta {
                continue;
            }

            // Parse arguments for `#[focus(when = some_condition)]`.
            let args = attr
                .parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)
                .map_err(|_| {
                    syn::Error::new_spanned(
                        attr,
                        "Invalid syntax for #[focus] attribute, expected #[focus(when = some_condition)]",
                    )
                })?;
            for arg in args {
                match arg {
                    Meta::NameValue(MetaNameValue { path, value, .. }) if path.is_ident("when") => {
                        when = Some(value);
                    }
                    other => {
                        return Err(syn::Error::new_spanned(
                            &other,
                            "Invalid argument for #[focus] attribute, expected key-value pairs like #[focus(when = some_condition)]",
                        ));
                    }
                }
            }
        }

        if found {
            let ident = field.ident.as_ref().ok_or_else(|| {
                syn::Error::new_spanned(field, "FocusSwitch only works on named fields with ident.")
            })?;
            let enum_field = ident.to_string().to_case(Case::Pascal);
            definitions.push(Definition {
                widget_name: ident.clone(),
                enum_field: Ident::new(&enum_field, Span::call_site()),
                when,
            });
        }
    }

    Ok(definitions)
}

/// Derives focus cycling, whole-view `SetFocus`/`IsFocus`, and event dispatch.
pub fn expand_focus(input: syn::DeriveInput) -> syn::Result<TokenStream> {
    let identifier = &input.ident;
    let (impl_generic, ty_generic, where_clause) = &input.generics.split_for_impl();

    let s = match &input.data {
        syn::Data::Struct(s) => s,
        _ => {
            return Err(syn::Error::new_spanned(
                identifier,
                "Focus can only be derived for structs",
            ));
        }
    };

    let definitions = collect_definitions(&s.fields)?;

    if definitions.is_empty() {
        return Err(syn::Error::new_spanned(
            identifier,
            "Focus derive requires at least one #[focus] field",
        ));
    }

    // Number of focusable fields.
    let def_len = definitions.len();

    // Generate enum name based on struct name.
    let enum_name = Ident::new(&format!("{identifier}Focus"), Span::call_site());

    // Create static array for indexing.
    let enum_fields = definitions.iter().map(|i| &i.enum_field);
    let impl_array = quote! {
        // Array for static indexing
        let focuses = [#(#enum_name::#enum_fields),*];
    };

    // Generate code for disabling current focus.
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

    // Generate code for enabling new focus.
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

    // Common code for both previous and next focus switching.
    let impl_general = quote! {
        #impl_array

        #impl_disable

        // Get index of current focus
        let index = focuses.iter().position(|f| *f == self.focus).unwrap();
    };

    // Forward and reverse traversal differ only by the per-step `delta` (forward = +1,
    // reverse = +(len-1), i.e. -1 mod len).
    let focus_loop = |delta: TokenStream| {
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

    // Generate Enum for focus states.
    let enum_fields = definitions.iter().map(|i| &i.enum_field);
    let enum_def = quote! {
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub enum #enum_name {
            #(#enum_fields),*
        }
    };

    // Implementation of HandleEvents.
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

    Ok(quote! {
        #enum_def
        #focus_def
        #set_focus_def
        #handle_def
    })
}

/// Appends the `focus`/`view_focused` fields the `Focus` derive needs.
pub fn expand_focusable(mut input: syn::DeriveInput) -> syn::Result<TokenStream> {
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

    let s = match &mut input.data {
        syn::Data::Struct(s) => s,
        _ => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "#[focusable] can only be applied to structs",
            ));
        }
    };

    let named = match &mut s.fields {
        Fields::Named(named) => named,
        _ => {
            return Err(syn::Error::new_spanned(
                &input.ident,
                "#[focusable] only works on structs with named fields.",
            ));
        }
    };

    let focus_ty = input.ident.to_string().to_case(Case::Pascal) + "Focus";
    let focus_field = Field {
        attrs: Vec::new(),
        mutability: syn::FieldMutability::None,
        vis: Visibility::Inherited,
        ident: Some(Ident::new("focus", Span::call_site())),
        colon_token: Some(Default::default()),
        ty: syn::parse_str::<Type>(&focus_ty)?,
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
        ty: syn::parse_str::<Type>("bool")?,
    };

    named.named.push(focus_field);
    named.named.push(view_focused_field);

    Ok(quote! { #input })
}

#[cfg(test)]
mod tests {
    use super::{expand_focus, expand_focusable};

    #[test]
    fn rejects_struct_with_no_focus_fields() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct EmptyView {
                focus: EmptyViewFocus,
                view_focused: bool,
            }
        };

        let err = expand_focus(input).expect_err("expected zero-field struct to be rejected");
        assert_eq!(
            err.to_string(),
            "Focus derive requires at least one #[focus] field"
        );
    }

    #[test]
    fn rejects_invalid_focus_attribute_syntax() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct TestView {
                #[focus(invalid syntax)]
                field: Widget,
            }
        };

        let err = expand_focus(input)
            .expect_err("expected invalid focus attribute syntax to be rejected");
        assert!(
            err.to_string()
                .contains("Invalid syntax for #[focus] attribute")
        );
    }

    #[test]
    fn rejects_unknown_focus_attribute_key() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct TestView {
                #[focus(unknown = some_value)]
                field: Widget,
            }
        };

        let err = expand_focus(input).expect_err("expected unknown focus key to be rejected");
        assert!(
            err.to_string()
                .contains("Invalid argument for #[focus] attribute")
        );
    }

    #[test]
    fn rejects_focus_on_unnamed_field() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct TestView(
                #[focus]
                Widget,
            );
        };

        let err = expand_focus(input).expect_err("expected unnamed field to be rejected");
        assert!(
            err.to_string()
                .contains("FocusSwitch only works on named fields with ident")
        );
    }

    #[test]
    fn rejects_focus_derive_on_enum() {
        let input: syn::DeriveInput = syn::parse_quote! {
            enum TestEnum {
                #[focus]
                Variant,
            }
        };

        let err = expand_focus(input).expect_err("expected enum to be rejected");
        assert!(
            err.to_string()
                .contains("Focus can only be derived for structs")
        );
    }

    #[test]
    fn rejects_focusable_on_enum() {
        let input: syn::DeriveInput = syn::parse_quote! {
            enum TestEnum {
                Variant,
            }
        };

        let err = expand_focusable(input).expect_err("expected focusable on enum to be rejected");
        assert!(
            err.to_string()
                .contains("#[focusable] can only be applied to structs")
        );
    }

    #[test]
    fn rejects_focusable_on_tuple_struct() {
        let input: syn::DeriveInput = syn::parse_quote! {
            struct TestView(Widget, Widget);
        };

        let err =
            expand_focusable(input).expect_err("expected focusable on tuple struct to be rejected");
        assert!(
            err.to_string()
                .contains("#[focusable] only works on structs with named fields")
        );
    }
}
