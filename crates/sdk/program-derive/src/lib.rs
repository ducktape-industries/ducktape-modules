//! `#[program]` on a state struct: the struct gains its store handle, `open`
//! (the root record under `state`, every collection under its field name),
//! `save`, `host` and `Default` (a fresh `MemoryStore`).
//!
//! `#[program]` on the struct's impl: every `pub fn` with a receiver is the
//! contract. `&mut self` methods are `Op` variants, `&self` methods `Query`
//! variants, named after the method in PascalCase and carrying its parameters
//! (minus `&Env`). `Reply` carries each query's return type (`T` of a
//! `Result<T, _>`, or the type itself). Behind `feature = "program"` on
//! wasm32 the impl also gets `guest::Program` (decode, dispatch, an op's
//! `T` through `output`, a query's `Reply` through `reply`, the root record
//! saved after an op) and `guest::program!`.
use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::{format_ident, quote};
use syn::{
    FnArg, GenericArgument, Ident, ImplItem, ItemImpl, ItemStruct, Pat, PathArguments, ReturnType,
    Type, Visibility, parse_quote,
};

#[proc_macro_attribute]
pub fn program(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let item: Tokens = item.into();
    if let Ok(structure) = syn::parse2::<ItemStruct>(item.clone()) {
        return state(structure).into();
    }
    match syn::parse2::<ItemImpl>(item) {
        Ok(implementation) => contract(implementation).into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn last_segment(ty: &Type) -> Option<&syn::PathSegment> {
    match ty {
        Type::Path(path) => path.path.segments.last(),
        Type::Reference(reference) => last_segment(&reference.elem),
        _ => None,
    }
}

fn names(ty: &Type, name: &str) -> bool {
    last_segment(ty).is_some_and(|segment| segment.ident == name)
}

/// `T` of `Result<T, _>`, or the type itself.
fn payload(ty: &Type) -> Type {
    if let Some(segment) = last_segment(ty)
        && segment.ident == "Result"
        && let PathArguments::AngleBracketed(args) = &segment.arguments
        && let Some(GenericArgument::Type(inner)) = args.args.first()
    {
        return inner.clone();
    }
    ty.clone()
}

fn is_unit(ty: &Type) -> bool {
    matches!(ty, Type::Tuple(tuple) if tuple.elems.is_empty())
}

fn pascal(ident: &Ident) -> Ident {
    let mut out = String::new();
    for part in ident.to_string().split('_') {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    format_ident!("{out}", span = ident.span())
}

fn state(mut structure: ItemStruct) -> Tokens {
    let name = &structure.ident;
    let syn::Fields::Named(fields) = &mut structure.fields else {
        panic!("#[program] takes a struct with named fields");
    };
    let mut collections = Vec::new();
    let mut scalars = Vec::new();
    let mut scalar_types = Vec::new();
    for field in &fields.named {
        let ident = field.ident.clone().unwrap();
        let ty = &field.ty;
        if names(ty, "Map") || names(ty, "Set") || names(ty, "Item") {
            let prefix = ident.to_string();
            collections.push(quote! { #ident: <#ty>::new(host.clone(), #prefix) });
        } else {
            scalars.push(ident);
            scalar_types.push(ty.clone());
        }
    }
    fields
        .named
        .push(parse_quote! { __host: ::program::Handle });
    let scalars = &scalars;
    let scalar_types = &scalar_types;
    quote! {
        #structure

        impl #name {
            /// The state over a store: the root record loaded (or every
            /// scalar at its default), every collection at its prefix.
            pub fn open(host: ::program::Handle) -> Result<Self, ::program::__private::abi::Refusal> {
                let (#(#scalars,)*): (#(#scalar_types,)*) = match host.get(::program::__private::STATE) {
                    Some(bytes) => ::program::__private::abi::decode(&bytes)
                        .map_err(|fault| ::program::corrupt(format!("the root record: {}", fault.sentence)))?,
                    None => Default::default(),
                };
                Ok(Self { #(#collections,)* #(#scalars,)* __host: host })
            }

            /// Writes the root record.
            pub fn save(&self) {
                let scalars = (#(&self.#scalars,)*);
                self.__host.set(
                    ::program::__private::STATE.to_vec(),
                    ::program::__private::abi::encode(&scalars),
                );
            }

            /// The store this state lives in.
            pub fn host(&self) -> &::program::Handle {
                &self.__host
            }
        }

        impl Default for #name {
            /// Fresh state over a `MemoryStore`.
            fn default() -> Self {
                Self::open(::program::MemoryStore::new()).unwrap()
            }
        }
    }
}

struct Method {
    ident: Ident,
    variant: Ident,
    fields: Vec<(Ident, Type)>,
    /// `env` or a field, in the method's parameter order.
    args: Vec<Tokens>,
    returns: Type,
    fallible: bool,
}

fn method(function: &syn::ImplItemFn) -> Option<(bool, Method)> {
    if !matches!(function.vis, Visibility::Public(_)) {
        return None;
    }
    let mut params = function.sig.inputs.iter();
    let FnArg::Receiver(receiver) = params.next()? else {
        return None;
    };
    let mut fields = Vec::new();
    let mut args = Vec::new();
    for param in params {
        let FnArg::Typed(typed) = param else { continue };
        if names(&typed.ty, "Env") {
            args.push(quote!(env));
            continue;
        }
        let Pat::Ident(pat) = &*typed.pat else {
            panic!("#[program] takes plain parameter names");
        };
        let ident = pat.ident.clone();
        args.push(quote!(#ident));
        fields.push((ident, (*typed.ty).clone()));
    }
    let ty = match &function.sig.output {
        ReturnType::Default => parse_quote!(()),
        ReturnType::Type(_, ty) => (**ty).clone(),
    };
    let fallible = names(&ty, "Result");
    Some((
        receiver.mutability.is_some(),
        Method {
            ident: function.sig.ident.clone(),
            variant: pascal(&function.sig.ident),
            fields,
            args,
            returns: payload(&ty),
            fallible,
        },
    ))
}

fn contract(implementation: ItemImpl) -> Tokens {
    let name = &implementation.self_ty;
    let mut ops = Vec::new();
    let mut queries = Vec::new();
    for item in &implementation.items {
        if let ImplItem::Fn(function) = item
            && let Some((mutates, method)) = method(function)
        {
            if mutates { &mut ops } else { &mut queries }.push(method);
        }
    }
    let variant = |m: &Method| {
        let (variant, fields) = (&m.variant, &m.fields);
        let fields = fields.iter().map(|(ident, ty)| quote!(#ident: #ty));
        quote! { #variant { #(#fields),* } }
    };
    let op_variants = ops.iter().map(variant);
    let query_variants = queries.iter().map(variant);
    let reply_variants = queries.iter().map(|m| {
        let (variant, ty) = (&m.variant, &m.returns);
        quote! { #variant(#ty) }
    });
    let op_arms = ops.iter().map(|m| {
        let Method {
            ident,
            variant,
            fields,
            args,
            returns,
            ..
        } = m;
        let fields = fields.iter().map(|(ident, _)| ident);
        let output = if is_unit(returns) {
            quote! { state.#ident(#(#args),*)?; }
        } else {
            quote! { ctx.output(::guest::abi::encode(&state.#ident(#(#args),*)?)); }
        };
        quote! { Op::#variant { #(#fields),* } => { #output } }
    });
    let query_arms = queries.iter().map(|m| {
        let Method {
            ident,
            variant,
            fields,
            args,
            fallible,
            ..
        } = m;
        let fields = fields.iter().map(|(ident, _)| ident);
        let ask = fallible.then(|| quote!(?));
        quote! { Query::#variant { #(#fields),* } => Reply::#variant(state.#ident(#(#args),*)#ask) }
    });
    let derives = quote! {
        #[derive(Clone, Debug, PartialEq, Eq, ::borsh::BorshSerialize, ::borsh::BorshDeserialize)]
    };
    quote! {
        #implementation

        #derives
        pub enum Op { #(#op_variants),* }

        #derives
        pub enum Query { #(#query_variants),* }

        #derives
        pub enum Reply { #(#reply_variants),* }

        #[cfg(all(feature = "program", target_arch = "wasm32"))]
        mod __program {
            use super::*;

            impl ::guest::Program for #name {
                fn execute(
                    ctx: &mut ::guest::Execute,
                    env: &::guest::abi::Env,
                    payload: &[u8],
                ) -> Result<(), ::guest::abi::Refusal> {
                    let _ = (&ctx, &env);
                    let op: Op = ::guest::abi::decode(payload)?;
                    let mut state = <#name>::open(::program::__private::Rc::new(::program::Host))?;
                    match op { #(#op_arms)* }
                    state.save();
                    Ok(())
                }

                fn query(
                    ctx: &mut ::guest::Query,
                    env: &::guest::abi::Env,
                    request: &[u8],
                ) -> Result<(), ::guest::abi::Refusal> {
                    let _ = &env;
                    let query: Query = ::guest::abi::decode(request)?;
                    let state = <#name>::open(::program::__private::Rc::new(::program::Host))?;
                    let reply = match query { #(#query_arms),* };
                    ctx.reply(&reply);
                    Ok(())
                }
            }

            ::guest::program!(#name);
        }
    }
}
