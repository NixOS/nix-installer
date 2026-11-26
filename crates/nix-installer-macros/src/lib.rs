/*! `#[step]` marks a struct as a step spec and derives its serde and
registry plumbing.

`name` is the serde tag (under the key `action_name`), the registry name and
the journal identity. A step must not carry its own container-level serde tag
or rename. The item's crate must depend on `nix-installer-kernel`.
*/

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse::Parse;
use syn::punctuated::Punctuated;
use syn::{Data, DeriveInput, Ident, LitStr, Token};

struct StepArgs {
    name: String,
    category: String,
    touches: Vec<String>,
    keep_effects: bool,
}

impl Parse for StepArgs {
    fn parse(input: syn::parse::ParseStream<'_>) -> syn::Result<Self> {
        let mut name: Option<String> = None;
        let mut category: Option<String> = None;
        let mut touches: Option<Vec<String>> = None;
        let mut revert: Option<bool> = None;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            let _eq: Token![=] = input.parse()?;
            let dup = |ident: &Ident| syn::Error::new(ident.span(), "`step` attribute set twice");
            match ident.to_string().as_str() {
                "name" if name.is_none() => name = Some(input.parse::<LitStr>()?.value()),
                "category" if category.is_none() => {
                    category = Some(input.parse::<LitStr>()?.value())
                },
                "touches" if touches.is_none() => {
                    let content;
                    syn::bracketed!(content in input);
                    let list = Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?;
                    touches = Some(list.iter().map(LitStr::value).collect());
                },
                "revert" if revert.is_none() => {
                    revert = Some(match input.parse::<LitStr>()?.value().as_str() {
                        "revert" => false,
                        "keep" => true,
                        other => {
                            return Err(syn::Error::new(
                                ident.span(),
                                format!(
                                    "unknown revert policy `{other}` \
                                     (expected `revert` or `keep`)"
                                ),
                            ));
                        },
                    });
                },
                "name" | "category" | "touches" | "revert" => return Err(dup(&ident)),
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown `step` attribute `{other}` \
                             (expected name, category, touches, revert)"
                        ),
                    ));
                },
            }
            if input.is_empty() {
                break;
            }
            let _comma: Token![,] = input.parse()?;
        }

        let name = name.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "`step` requires `name = \"...\"`")
        })?;
        let category = category.ok_or_else(|| {
            syn::Error::new(Span::call_site(), "`step` requires `category = \"...\"`")
        })?;
        Ok(StepArgs {
            name,
            category,
            touches: touches.unwrap_or_default(),
            keep_effects: revert.unwrap_or(false),
        })
    }
}

/// Declares a serializable step spec.
///
/// `name` and `category` are required. `touches` and `revert` are optional.
///
/// ```ignore
/// #[step(name = "create_user", category = "users", revert = "keep")]
/// struct CreateUserSpec {
///     name: String,
///     uid: u32,
/// }
/// ```
#[proc_macro_attribute]
pub fn step(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = syn::parse_macro_input!(attr as StepArgs);
    let input = syn::parse_macro_input!(item as DeriveInput);

    if !input.generics.params.is_empty() {
        return syn::Error::new_spanned(
            &input.generics,
            "`#[step]` does not support generic steps; specs own their data",
        )
        .to_compile_error()
        .into();
    }
    // An internally-tagged serde derive needs named fields, so a fieldless
    // step cannot carry the tag.
    let named_fields = matches!(&input.data,
        Data::Struct(data) if matches!(data.fields, syn::Fields::Named(_)));
    if !named_fields {
        return syn::Error::new_spanned(
            &input.ident,
            "`#[step]` requires a struct with named fields",
        )
        .to_compile_error()
        .into();
    }

    let ident = &input.ident;
    let name = LitStr::new(&args.name, Span::call_site());
    let category = LitStr::new(&args.category, Span::call_site());
    let touches: Vec<LitStr> = args
        .touches
        .iter()
        .map(|t| LitStr::new(t, Span::call_site()))
        .collect();
    let revert = if args.keep_effects {
        quote! { ::nix_installer_kernel::RevertPolicy::Keep }
    } else {
        quote! { ::nix_installer_kernel::RevertPolicy::Revert }
    };

    let expanded = quote! {
        #[derive(
            ::core::fmt::Debug,
            ::core::clone::Clone,
            ::serde::Serialize,
            ::serde::Deserialize
        )]
        #[serde(tag = "action_name", rename = #name)]
        #input

        impl ::nix_installer_kernel::StepDecl for #ident {
            const META: ::nix_installer_kernel::StepMeta =
                ::nix_installer_kernel::StepMeta {
                    name: #name,
                    category: #category,
                    touches: &[#(#touches),*],
                    revert: #revert,
                };
        }
    };
    expanded.into()
}
