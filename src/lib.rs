#![allow(clippy::enum_glob_use)]

/*!

# `facile`: build traits from their implementations

## Why?

In writing production Rust it's common to have a type that bundles a number of
different types, each of which expects some set of trait bounds in order to
‘function’ (have basic functionality implemented on it). Naïvely, this results
in propagating a large number of potentially quite verbose trait bounds
arbitrarily far up the program:

```
struct Foo<P, F> {
    pointer: P,
    future: F,
}

impl<P: AsRef<str>, F: Future<Output = ()> + Clone> Foo<P, F> {
    async fn run(&self) {
        println!("running future {}", self.pointer.as_ref());
        self.future.clone().await
    }
}

// These bounds must be propagated all the way up to the constructon of the
// `Foo`, where they are made concrete, repeated at every point. If the `impl`
// on `Foo` change constraints, all of these sites must change, even though
// they're just passing the constraints through.
async fn use_foo<P: AsRef<str>, F: Future<Output = ()> + Clone>(foo: &Foo<P, F>) {
    foo.run().await
}
```

You can fix this by replacing the type with a trait that bundles together the
constraints:

```
struct FooImpl<P, F> {
    pointer: P,
    future: F,
}

trait Foo {
    async fn run(&self);
}

impl<P: AsRef<str>, F: Future<Output = ()> + Clone> Foo for FooImpl<P, F> {
    async fn run(&self) {
        println!("running future {}", self.pointer.as_ref());
        self.future.clone().await
    }
}

// `impl Trait` syntax can often be used to make the resulting code even simpler.
async fn use_foo(foo: &impl Foo) {
    foo.run().await
}
```

but this can be annoying, especially for larger APIs, as it requires duplicating
the signatures of the entire façade.

Instead, this crate provides the `facade` attribute: write the implementation of
the façade as if you were implementing a non-existent trait, and the trait
itself will be auto-generated for you, bundling together the constraints and
implementation.

```
# struct FooImpl<P, F> {
#     pointer: P,
#     future: F,
# }

#[facile::facade]
impl<P: AsRef<str>, F: Future<Output = ()> + Clone> Foo for FooImpl<P, F> {
    async fn run(&self) {
        println!("running future {}", self.pointer.as_ref());
        self.future.clone().await
    }
}

async fn use_foo(foo: &impl Foo) {
    foo.run().await
}
```
*/

use proc_macro::TokenStream;
use quote::quote;

use syn::punctuated::Punctuated;

#[derive(deluxe::ParseMetaItem)]
struct Attrs {
    #[deluxe(default)]
    visibility: Option<syn::Visibility>,
    #[deluxe(default)]
    r#where: Vec<syn::WherePredicate>,
}

/**
Construct a façade trait based on an implementation.

Given an implementation of a non-existent trait, construct a trait by that name
from the signature of the implementation.

# Parameters

## `visibility`

The visibility of the resulting trait, e.g. `pub` or `pub(super)`. If no
visibility is provided then no explicit visibility will be applied to the trait.

## `where`

A list of where predicates to apply to the trait definition, e.g. `T: Clone`,
`Self: Send`. Generics on the implementation (`impl<…> where …`) apply to the
_implementation_ rather than the trait. If you want to constrain the trait
itself, use this parameter. Bounds on `Self` can be used to provide supertraits
(which are otherwise not supported by the syntax).

# Panics

If the input is not a trait implementation.

# Type parameters and associated types

Type or lifetime parameters on the trait will be automatically lifted
to the trait definition, and can be constrained with `where`.

Since type parameters on the type aren't in scope for the trait, you must assign
them to an associated type and reference them through that (as `Self::AssocType`).

In an extension of the usual impl syntax, associated types can be given both
bounds (for the trait definitiion) and definitions (for the implementation), e.g.

```ignore
type Future: Future = std::future::Ready<()>;
```

# Example

Put the `facile::facade` annotation on a trait implementation with a
non-existent trait name to generate a trait by that name and also implement it
as written.

```
struct FooImpl<F, P> {
    future: F,
    pointer: P,
}

#[facile::facade(
    visibility = pub(crate),
    where(F: Future<Output = ()>),
)]
impl<F, P> Foo<F> for FooImpl<F, P>
where
    F: Future<Output = ()> + Clone,
    P: AsRef<str>,
{
    type Future: Future<Output = ()> = F;

    fn future(&self) -> Self::Future {
        self.future.clone()
    }

    fn as_str(&self) -> &str {
        self.pointer.as_ref()
    }
}

async fn use_foo(foo: impl Foo<std::future::Ready<()>>) {
    println!("running future: {}", foo.as_str());
    foo.future().await
}
```
 */
#[proc_macro_attribute]
#[allow(clippy::too_many_lines)]
pub fn facade(args: TokenStream, item: TokenStream) -> TokenStream {
    let args: Attrs = deluxe::parse(args).unwrap();
    let mut r#impl = syn::parse_macro_input!(item as syn::ItemImpl);

    let (_, mut trait_path, _) = r#impl.trait_.clone().expect("trait should be named");
    assert!(trait_path.segments.len() == 1, "trait identifier may not contain a path");

    let segment = trait_path.segments.pop().unwrap().into_value();
    let trait_ident = segment.ident;

    let mut generics = match segment.arguments {
        syn::PathArguments::None => syn::Generics::default(),
        syn::PathArguments::AngleBracketed(trait_args) => {
            let mut generics = syn::Generics {
                lt_token: Some(trait_args.lt_token),
                gt_token: Some(trait_args.gt_token),
                where_clause: None,
                params: Punctuated::default(),
            };
            for arg in trait_args.args {
                use syn::GenericArgument::*;
                match arg {
                    Type(syn::Type::Path(type_)) => {
                        generics
                            .params
                            .push(syn::GenericParam::Type(syn::TypeParam {
                                attrs: vec![],
                                ident: type_
                                    .path
                                    .get_ident()
                                    .expect("type parameters must be idents")
                                    .clone(),
                                colon_token: None,
                                bounds: Punctuated::default(),
                                eq_token: None,
                                default: None,
                            }));
                    }
                    Lifetime(lifetime) => {
                        generics.params.push(syn::GenericParam::Lifetime(syn::LifetimeParam {
                            attrs: vec![],
                            lifetime,
                            colon_token: None,
                            bounds: Punctuated::default(),
                        }));
                    }
                    _ => panic!("only type and lifetime parameters are supported on traits"),
                }
            }
            generics
        }
        syn::PathArguments::Parenthesized(_) => panic!("trait arguments must be angle-bracketed"),
    };

    generics.make_where_clause().predicates.extend(args.r#where);

    let mut r#trait = syn::ItemTrait {
        attrs: r#impl.attrs.clone(),
        vis: args.visibility.unwrap_or(syn::Visibility::Inherited),
        unsafety: None,
        auto_token: None,
        restriction: None,
        trait_token: <syn::Token![trait]>::default(),
        ident: trait_ident,
        generics,
        colon_token: None,
        supertraits: syn::punctuated::Punctuated::default(),
        brace_token: r#impl.brace_token,
        items: vec![],
    };

    for item in &mut r#impl.items {
        use syn::ImplItem::*;

        r#trait.items.push(match item {
            Const(item) => {
                let item = item.clone();
                syn::TraitItem::Const(syn::TraitItemConst {
                    attrs: item.attrs,
                    const_token: item.const_token,
                    ident: item.ident,
                    generics: item.generics,
                    colon_token: item.colon_token,
                    ty: item.ty,
                    default: None,
                    semi_token: item.semi_token,
                })
            }
            Fn(item) => {
                let item = item.clone();
                syn::TraitItem::Fn(syn::TraitItemFn {
                    attrs: item.attrs,
                    sig: item.sig,
                    default: None,
                    semi_token: Some(<syn::Token![;]>::default()),
                })
            }
            Type(item) => {
                let item = item.clone();
                syn::TraitItem::Type(syn::TraitItemType {
                    attrs: item.attrs,
                    type_token: item.type_token,
                    ident: item.ident,
                    generics: item.generics,
                    default: None,
                    semi_token: item.semi_token,
                    colon_token: None,
                    bounds: Punctuated::default(),
                })
            }
            impl_item => {
                let Verbatim(stream) = impl_item else {
                    unimplemented!()
                };
                let stream: TokenStream = stream.clone().into();
                let item = syn::parse_macro_input!(stream as syn::TraitItemType);
                let mut decl = item.clone();
                let (eq_token, ty) = decl
                    .default
                    .take()
                    .unwrap_or_else(|| panic!("need definition for type {}", item.ident));
                *impl_item = syn::ImplItem::Type(syn::ImplItemType {
                    attrs: item.attrs,
                    vis: syn::Visibility::Inherited,
                    defaultness: None,
                    type_token: item.type_token,
                    ident: item.ident,
                    generics: item.generics,
                    eq_token,
                    ty,
                    semi_token: item.semi_token,
                });
                syn::TraitItem::Type(decl)
            }
        });
    }

    quote!(#r#trait #r#impl).into()
}
