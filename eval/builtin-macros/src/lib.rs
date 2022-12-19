extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{quote, quote_spanned, ToTokens};
use syn::parse::Parse;
use syn::spanned::Spanned;
use syn::{
    parse2, parse_macro_input, parse_quote, Attribute, FnArg, Ident, Item, ItemMod, LitStr, Pat,
    PatIdent, PatType, Token,
};

struct BuiltinArgs {
    name: LitStr,
}

impl Parse for BuiltinArgs {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        Ok(BuiltinArgs {
            name: input.parse()?,
        })
    }
}

fn extract_docstring(attrs: &[Attribute]) -> Option<String> {
    // Rust docstrings are transparently written pre-macro expansion into an attribute that looks
    // like:
    //
    // #[doc = "docstring here"]
    //
    // Multi-line docstrings yield multiple attributes in order, which we assemble into a single
    // string below.

    #[allow(dead_code)]
    #[derive(Debug)]
    struct Docstring {
        eq: Token![=],
        doc: LitStr,
    }

    impl Parse for Docstring {
        fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
            Ok(Self {
                eq: input.parse()?,
                doc: input.parse()?,
            })
        }
    }

    attrs
        .iter()
        .filter(|attr| attr.path.get_ident().into_iter().any(|id| id == "doc"))
        .filter_map(|attr| parse2::<Docstring>(attr.tokens.clone()).ok())
        .map(|docstring| docstring.doc.value())
        .reduce(|mut fst, snd| {
            if snd.is_empty() {
                // An empty string represents a spacing newline that was added in the
                // original doc comment.
                fst.push_str("\n\n");
            } else {
                fst.push_str(&snd);
            }

            fst
        })
}

/// Mark the annotated module as a module for defining Nix builtins.
///
/// A function `fn builtins() -> Vec<Builtin>` will be defined within the annotated module,
/// returning a list of [`tvix_eval::Builtin`] for each function annotated with the `#[builtin]`
/// attribute within the module.
///
/// Each invocation of the `#[builtin]` annotation within the module should be passed a string
/// literal for the name of the builtin.
///
/// # Examples
/// ```ignore
/// # use tvix_eval_builtin_macros::builtins;
/// # mod value {
/// #     pub use tvix_eval::Builtin;
/// # }
///
/// #[builtins]
/// mod builtins {
///     use tvix_eval::{ErrorKind, Value, VM};
///
///     #[builtin("identity")]
///     pub fn builtin_identity(_vm: &mut VM, x: Value) -> Result<Value, ErrorKind> {
///         Ok(x)
///     }
///
///     // Builtins can request their argument not be forced before being called by annotating the
///     // argument with the `#[lazy]` attribute
///
///     #[builtin("tryEval")]
///     pub fn builtin_try_eval(vm: &mut VM, #[lazy] x: Value) -> Result<Value, ErrorKind> {
///         todo!()
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn builtins(_args: TokenStream, item: TokenStream) -> TokenStream {
    let mut module = parse_macro_input!(item as ItemMod);

    let (_, items) = match &mut module.content {
        Some(content) => content,
        None => {
            return (quote_spanned!(module.span() =>
                compile_error!("Builtin modules must be defined in-line")
            ))
            .into();
        }
    };

    let mut builtins = vec![];
    for item in items.iter_mut() {
        if let Item::Fn(f) = item {
            if let Some(builtin_attr_pos) = f
                .attrs
                .iter()
                .position(|attr| attr.path.get_ident().iter().any(|id| *id == "builtin"))
            {
                let builtin_attr = f.attrs.remove(builtin_attr_pos);
                let BuiltinArgs { name } = match builtin_attr.parse_args() {
                    Ok(args) => args,
                    Err(err) => return err.into_compile_error().into(),
                };

                if f.sig.inputs.len() <= 1 {
                    return (quote_spanned!(
                        f.sig.inputs.span() =>
                            compile_error!("Builtin functions must take at least two arguments")
                    ))
                    .into();
                }

                let builtin_arguments = f
                    .sig
                    .inputs
                    .iter_mut()
                    .skip(1)
                    .map(|arg| {
                        let mut strict = true;
                        let name = match arg {
                            FnArg::Receiver(_) => {
                                return Err(quote_spanned!(arg.span() => {
                                    compile_error!("Unexpected receiver argument in builtin")
                                }))
                            }
                            FnArg::Typed(PatType { attrs, pat, .. }) => {
                                attrs.retain(|attr| {
                                    attr.path.get_ident().into_iter().any(|id| {
                                        if id == "lazy" {
                                            strict = false;
                                            false
                                        } else {
                                            true
                                        }
                                    })
                                });
                                match pat.as_ref() {
                                    Pat::Ident(PatIdent { ident, .. }) => ident.to_string(),
                                    _ => "unknown".to_string(),
                                }
                            }
                        };

                        Ok(quote_spanned!(arg.span() => {
                            crate::internal::BuiltinArgument {
                                strict: #strict,
                                name: #name,
                            }
                        }))
                    })
                    .collect::<Result<Vec<_>, _>>();

                let builtin_arguments = match builtin_arguments {
                    Ok(args) => args,
                    Err(err) => return err.into(),
                };

                let fn_name = f.sig.ident.clone();
                let num_args = f.sig.inputs.len() - 1;
                let args = (0..num_args)
                    .map(|n| Ident::new(&format!("arg_{n}"), Span::call_site()))
                    .collect::<Vec<_>>();
                let mut reversed_args = args.clone();
                reversed_args.reverse();

                let docstring = match extract_docstring(&f.attrs) {
                    Some(docs) => quote!(Some(#docs)),
                    None => quote!(None),
                };

                builtins.push(quote_spanned! { builtin_attr.span() => {
                    crate::internal::Builtin::new(
                        #name,
                        &[#(#builtin_arguments),*],
                        #docstring,
                        |mut args: Vec<crate::Value>, vm: &mut crate::internal::VM| {
                            #(let #reversed_args = args.pop().unwrap();)*
                            #fn_name(vm, #(#args),*)
                        }
                    )
                }});
            }
        }
    }

    items.push(parse_quote! {
        pub fn builtins() -> Vec<crate::internal::Builtin> {
            vec![#(#builtins),*]
        }
    });

    module.into_token_stream().into()
}
