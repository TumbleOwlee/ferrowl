//! `#[derive(Overlay)]` — structural helpers + common-key routing for overlay enums.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, Ident};

/// Derives `is_active`/`close`/`take`/`route_keys` for a mutually-exclusive overlay enum.
pub fn expand_overlay(input: syn::DeriveInput) -> syn::Result<TokenStream> {
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
    let mut arms: Vec<TokenStream> = vec![];

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
