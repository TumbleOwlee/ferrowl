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

            // Generate implementation for focus switching methods
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
                #handle_def
            })
        }
        _ => unimplemented!("State not implemented for type"),
    }
}

#[proc_macro_attribute]
pub fn focusable(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = syn::parse_macro_input!(item as syn::DeriveInput);

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

            match &mut s.fields {
                Fields::Named(named) => {
                    named.named.push(focus_field);
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
