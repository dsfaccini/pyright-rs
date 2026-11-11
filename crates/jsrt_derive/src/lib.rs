//! `#[derive(Trace)]` for `jsrt::Trace`: lets the evaluator heap collector
//! find every handle a value holds. See `docs/collector-design.md`.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Attribute, Data, DeriveInput, Fields, parse_macro_input, parse_quote};

/// Container attributes: `#[trace(eval)]` (the type may be allocated with
/// `P::new_eval`) and `#[trace(holds_eval)]` (force `HOLDS_EVAL`, for a type
/// that contains itself without a `P`).
/// Field attributes: `#[trace(skip)]` (opaque, never traced) and
/// `#[trace(program)]` (holds program-heap handles only; checked in check mode).
#[proc_macro_derive(Trace, attributes(trace))]
pub fn derive_trace(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand(input).unwrap_or_else(|e| e.to_compile_error()).into()
}

#[derive(Default)]
struct Flags {
    eval: bool,
    holds_eval: bool,
    skip: bool,
    program: bool,
}

fn flags(attrs: &[Attribute]) -> syn::Result<Flags> {
    let mut flags = Flags::default();
    for attr in attrs.iter().filter(|a| a.path().is_ident("trace")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("eval") {
                flags.eval = true;
            } else if meta.path.is_ident("holds_eval") {
                flags.holds_eval = true;
            } else if meta.path.is_ident("skip") {
                flags.skip = true;
            } else if meta.path.is_ident("program") {
                flags.program = true;
            } else {
                return Err(meta.error("expected eval, holds_eval, skip or program"));
            }
            Ok(())
        })?;
    }
    Ok(flags)
}

/// The match pattern binding every field, and the statements tracing them.
/// The consts' terms each field contributes.
#[derive(Default)]
struct Consts {
    holds: Vec<TokenStream2>,
    cells: Vec<TokenStream2>,
    outs: Vec<TokenStream2>,
}

/// The match pattern binding every field, the statements tracing them, and the
/// statements prefetching their owned buffers.
fn fields(fields: &Fields, consts: &mut Consts) -> syn::Result<(TokenStream2, TokenStream2, TokenStream2)> {
    let mut binds = Vec::new();
    let mut body = Vec::new();
    let mut prefetch = Vec::new();
    for (i, field) in fields.iter().enumerate() {
        let f = flags(&field.attrs)?;
        let bind = format_ident!("__f{i}");
        let ty = &field.ty;
        match &field.ident {
            Some(name) => binds.push(quote!(#name: #bind)),
            None => binds.push(quote!(#bind)),
        }
        if f.skip {
            continue;
        }
        if f.program {
            body.push(quote! {
                if __t.checking() { __t.program_only(#bind); }
            });
            continue;
        }
        prefetch.push(quote!(::jsrt::Trace::prefetch_owned(#bind);));
        consts.holds.push(quote!(<#ty as ::jsrt::Trace>::HOLDS_EVAL));
        consts.cells.push(quote!(<#ty as ::jsrt::Trace>::HAS_CELL));
        consts.outs.push(quote!(<#ty as ::jsrt::Trace>::CELL_OUT_OF_LINE));
        body.push(quote! {
            if <#ty as ::jsrt::Trace>::HOLDS_EVAL || __t.checking() { ::jsrt::Trace::trace(#bind, __t); }
        });
    }
    let pattern = match fields {
        Fields::Named(_) => quote!({ #(#binds,)* }),
        Fields::Unnamed(_) => quote!(( #(#binds,)* )),
        Fields::Unit => quote!(),
    };
    Ok((pattern, quote!(#(#body)*), quote!(#(#prefetch)*)))
}

fn expand(mut input: DeriveInput) -> syn::Result<TokenStream2> {
    let container = flags(&input.attrs)?;
    let name = &input.ident;
    let mut consts = Consts::default();
    let mut prefetch_arms = Vec::new();
    let arms = match &input.data {
        Data::Struct(data) => {
            let (pattern, body, prefetch) = fields(&data.fields, &mut consts)?;
            prefetch_arms.push(quote!(Self #pattern => { #prefetch }));
            vec![quote!(Self #pattern => { #body })]
        }
        Data::Enum(data) => {
            let mut arms = Vec::new();
            for variant in &data.variants {
                let ident = &variant.ident;
                let (pattern, body, prefetch) = fields(&variant.fields, &mut consts)?;
                prefetch_arms.push(quote!(Self::#ident #pattern => { #prefetch }));
                arms.push(quote!(Self::#ident #pattern => { #body }));
            }
            arms
        }
        Data::Union(_) => return Err(syn::Error::new_spanned(name, "Trace cannot be derived for a union")),
    };
    let Consts { holds, cells, outs } = consts;
    let holds_eval = if container.holds_eval { quote!(true) } else { quote!(false #(|| #holds)*) };
    let eval = container.eval;
    // `#[trace(holds_eval)]` breaks a const cycle, so the type claims no cells:
    // such a type must hold none, which `trace` asserts at compile time.
    let has_cell = if container.holds_eval { quote!(false) } else { quote!(false #(|| #cells)*) };
    let out_of_line = if container.holds_eval { quote!(false) } else { quote!(false #(|| #outs)*) };
    let no_cells = if container.holds_eval {
        quote!(const { assert!(!(false #(|| #cells)*), "a #[trace(holds_eval)] type holds a cell that can hold an evaluator handle") };)
    } else {
        quote!()
    };

    // A generic type has no per-instantiation static; the heap falls back to a TypeId map.
    let class_slot = if input.generics.params.is_empty() {
        quote! {
            #[inline]
            fn class_slot() -> ::core::option::Option<&'static ::jsrt::ClassSlot> {
                static SLOT: ::jsrt::ClassSlot = ::jsrt::ClassSlot::new();
                ::core::option::Option::Some(&SLOT)
            }
        }
    } else {
        quote!()
    };
    let params: Vec<_> = input.generics.type_params().map(|p| p.ident.clone()).collect();
    let where_clause = input.generics.make_where_clause();
    for param in params {
        where_clause.predicates.push(parse_quote!(#param: ::jsrt::Trace));
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        #[automatically_derived]
        unsafe impl #impl_generics ::jsrt::Trace for #name #ty_generics #where_clause {
            const HOLDS_EVAL: bool = #holds_eval;
            const EVAL: bool = #eval;
            const HAS_CELL: bool = #has_cell;
            const CELL_OUT_OF_LINE: bool = #out_of_line;
            #class_slot
            #[allow(unused_variables)]
            fn trace(&self, __t: &mut ::jsrt::Tracer) {
                #no_cells
                match self {
                    #(#arms)*
                }
            }
            #[allow(unused_variables)]
            #[inline]
            fn prefetch_owned(&self) {
                match self {
                    #(#prefetch_arms)*
                }
            }
        }
    })
}
