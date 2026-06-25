//! `#[derive(TableEntry)]` and its companion `Header` ZST.

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Fields, Ident};

/// Derives `TableEntry` for a row struct plus a companion `Header` unit struct.
pub fn expand_table_entry(input: syn::DeriveInput) -> syn::Result<TokenStream> {
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
        header_ident.unwrap_or_else(|| Ident::new(&format!("{ident}Header"), ident.span()));

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
