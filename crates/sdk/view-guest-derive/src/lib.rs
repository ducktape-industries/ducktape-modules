use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, parse_macro_input};

#[proc_macro_derive(IntoElement)]
pub fn derive_into_element(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let generics = input.generics;
    if !matches!(input.data, Data::Struct(_)) {
        return syn::Error::new_spanned(name, "IntoElement requires a struct")
            .to_compile_error()
            .into();
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics ::ducktape_view_guest::IntoElement for #name #ty_generics #where_clause {
            type Element = Self;

            fn into_element(self) -> Self::Element {
                self
            }
        }

        impl #impl_generics ::ducktape_view_guest::Element for #name #ty_generics #where_clause {
            fn lower(
                self: ::std::boxed::Box<Self>,
                lowering: &mut ::ducktape_view_guest::Lowering<'_>,
            ) -> ::ducktape_view_guest::wire::Node {
                lowering.render_once(*self)
            }
        }
    }
    .into()
}
