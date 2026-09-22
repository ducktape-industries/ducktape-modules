use proc_macro::TokenStream;
use quote::quote;
use syn::{DeriveInput, parse_macro_input, parse_quote};

#[proc_macro_derive(IntoElement)]
pub fn derive_into_element(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;
    let mut generics = input.generics;
    generics
        .make_where_clause()
        .predicates
        .push(parse_quote!(Self: ::ducktape_view_guest::RenderOnce));
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    quote! {
        impl #impl_generics ::ducktape_view_guest::IntoElement for #name #ty_generics #where_clause {
            type Element = ::ducktape_view_guest::ViewElement<Self>;

            #[track_caller]
            fn into_element(self) -> Self::Element {
                ::ducktape_view_guest::ViewElement::new(self)
            }

            #[track_caller]
            #[inline(never)]
            fn into_any_element(self) -> ::ducktape_view_guest::AnyElement {
                ::ducktape_view_guest::Element::into_any(self.into_element())
            }
        }

        impl #impl_generics ::ducktape_view_guest::FluentBuilder for #name #ty_generics #where_clause {}
    }
    .into()
}
