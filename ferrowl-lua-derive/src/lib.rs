//! Proc macro for ferrowl-lua host modules.
//!
//! `#[derive(Module)] #[module = "C_Log"]` generates the trivial
//! `impl ferrowl_lua::module::Module { fn module() -> &'static str }`, working
//! through generics (e.g. `Log<S>`, `Register<T>`).

extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{Expr, ExprLit, Lit};

/// Derives [`Module`](ferrowl_lua::module::Module) from a `#[module = "…"]`
/// attribute supplying the Lua global name.
#[proc_macro_derive(Module, attributes(module))]
pub fn derive_module(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::DeriveInput);
    expand_module(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn expand_module(input: syn::DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = input.ident.clone();
    let generics = input.generics.clone();
    let (impl_generic, ty_generic, where_clause) = generics.split_for_impl();

    let mut name: Option<syn::LitStr> = None;
    for attr in &input.attrs {
        if !attr.path().is_ident("module") {
            continue;
        }
        let nv = attr.meta.require_name_value()?;
        match &nv.value {
            Expr::Lit(ExprLit {
                lit: Lit::Str(s), ..
            }) => name = Some(s.clone()),
            other => {
                return Err(syn::Error::new_spanned(
                    other,
                    "`#[module = …]` expects a string literal, e.g. `#[module = \"C_Log\"]`",
                ));
            }
        }
    }

    let name = name.ok_or_else(|| {
        syn::Error::new_spanned(
            &ident,
            "Module derive requires a `#[module = \"…\"]` attribute",
        )
    })?;

    Ok(quote! {
        impl #impl_generic ferrowl_lua::module::Module for #ident #ty_generic #where_clause {
            fn module() -> &'static str {
                #name
            }
        }
    })
}
