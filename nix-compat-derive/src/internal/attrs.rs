use quote::ToTokens;
use syn::meta::ParseNestedMeta;
use syn::parse::Parse;
use syn::{parse_quote, Attribute, Expr, ExprLit, ExprPath, Lit, Token};

use super::symbol::{Symbol, CRATE, DEFAULT, FROM, FROM_STR, NIX, TRY_FROM, VERSION};
use super::Context;

#[derive(Debug, PartialEq, Eq)]
pub enum Default {
    None,
    #[allow(clippy::enum_variant_names)]
    Default,
    Path(ExprPath),
}

impl Default {
    pub fn is_none(&self) -> bool {
        matches!(self, Default::None)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Field {
    pub default: Default,
    pub version: Option<syn::ExprRange>,
}

impl Field {
    pub fn from_ast(ctx: &Context, attrs: &Vec<Attribute>) -> Field {
        let mut version = None;
        let mut default = Default::None;
        for attr in attrs {
            if attr.path() != NIX {
                continue;
            }
            if let Err(err) = attr.parse_nested_meta(|meta| {
                if meta.path == VERSION {
                    version = parse_lit(ctx, &meta, VERSION)?;
                } else if meta.path == DEFAULT {
                    if meta.input.peek(Token![=]) {
                        if let Some(path) = parse_lit(ctx, &meta, DEFAULT)? {
                            default = Default::Path(path);
                        }
                    } else {
                        default = Default::Default;
                    }
                } else {
                    let path = meta.path.to_token_stream().to_string();
                    return Err(meta.error(format_args!("unknown nix field attribute '{}'", path)));
                }
                Ok(())
            }) {
                eprintln!("{:?}", err.span().source_text());
                ctx.syn_error(err);
            }
        }
        if version.is_some() && default.is_none() {
            default = Default::Default;
        }

        Field { default, version }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Variant {
    pub version: syn::ExprRange,
}

impl Variant {
    pub fn from_ast(ctx: &Context, attrs: &Vec<Attribute>) -> Variant {
        let mut version = parse_quote!(..);
        for attr in attrs {
            if attr.path() != NIX {
                continue;
            }
            if let Err(err) = attr.parse_nested_meta(|meta| {
                if meta.path == VERSION {
                    if let Some(v) = parse_lit(ctx, &meta, VERSION)? {
                        version = v;
                    }
                } else {
                    let path = meta.path.to_token_stream().to_string();
                    return Err(
                        meta.error(format_args!("unknown nix variant attribute '{}'", path))
                    );
                }
                Ok(())
            }) {
                eprintln!("{:?}", err.span().source_text());
                ctx.syn_error(err);
            }
        }

        Variant { version }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Container {
    pub from_str: Option<syn::Path>,
    pub type_from: Option<syn::Type>,
    pub type_try_from: Option<syn::Type>,
    pub crate_path: Option<syn::Path>,
}

impl Container {
    pub fn from_ast(ctx: &Context, attrs: &Vec<Attribute>) -> Container {
        let mut type_from = None;
        let mut type_try_from = None;
        let mut crate_path = None;
        let mut from_str = None;

        for attr in attrs {
            if attr.path() != NIX {
                continue;
            }
            if let Err(err) = attr.parse_nested_meta(|meta| {
                if meta.path == FROM {
                    type_from = parse_lit(ctx, &meta, FROM)?;
                } else if meta.path == TRY_FROM {
                    type_try_from = parse_lit(ctx, &meta, TRY_FROM)?;
                } else if meta.path == FROM_STR {
                    from_str = Some(meta.path);
                } else if meta.path == CRATE {
                    crate_path = parse_lit(ctx, &meta, CRATE)?;
                } else {
                    let path = meta.path.to_token_stream().to_string();
                    return Err(
                        meta.error(format_args!("unknown nix variant attribute '{}'", path))
                    );
                }
                Ok(())
            }) {
                eprintln!("{:?}", err.span().source_text());
                ctx.syn_error(err);
            }
        }

        Container {
            from_str,
            type_from,
            type_try_from,
            crate_path,
        }
    }
}

pub fn get_lit_str(
    ctx: &Context,
    meta: &ParseNestedMeta,
    attr: Symbol,
) -> syn::Result<Option<syn::LitStr>> {
    let expr: Expr = meta.value()?.parse()?;
    let mut value = &expr;
    while let Expr::Group(e) = value {
        value = &e.expr;
    }
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(s), ..
    }) = value
    {
        Ok(Some(s.clone()))
    } else {
        ctx.error_spanned(
            expr,
            format_args!("expected nix attribute {} to be string", attr),
        );
        Ok(None)
    }
}

pub fn parse_lit<T: Parse>(
    ctx: &Context,
    meta: &ParseNestedMeta,
    attr: Symbol,
) -> syn::Result<Option<T>> {
    match get_lit_str(ctx, meta, attr)? {
        Some(lit) => Ok(Some(lit.parse()?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod test {
    use syn::{parse_quote, Attribute};

    use crate::internal::Context;

    use super::*;

    #[test]
    fn parse_field_version() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[nix(version="..34")])];
        let ctx = Context::new();
        let field = Field::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            field,
            Field {
                default: Default::Default,
                version: Some(parse_quote!(..34)),
            }
        );
    }

    #[test]
    fn parse_field_default() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[nix(default)])];
        let ctx = Context::new();
        let field = Field::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            field,
            Field {
                default: Default::Default,
                version: None,
            }
        );
    }

    #[test]
    fn parse_field_default_path() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[nix(default="Default::default")])];
        let ctx = Context::new();
        let field = Field::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            field,
            Field {
                default: Default::Path(parse_quote!(Default::default)),
                version: None,
            }
        );
    }

    #[test]
    fn parse_field_both() {
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[nix(version="..", default="Default::default")])];
        let ctx = Context::new();
        let field = Field::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            field,
            Field {
                default: Default::Path(parse_quote!(Default::default)),
                version: Some(parse_quote!(..)),
            }
        );
    }

    #[test]
    fn parse_field_both_rev() {
        let attrs: Vec<Attribute> =
            vec![parse_quote!(#[nix(default="Default::default", version="..")])];
        let ctx = Context::new();
        let field = Field::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            field,
            Field {
                default: Default::Path(parse_quote!(Default::default)),
                version: Some(parse_quote!(..)),
            }
        );
    }

    #[test]
    fn parse_field_no_attr() {
        let attrs: Vec<Attribute> = vec![];
        let ctx = Context::new();
        let field = Field::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            field,
            Field {
                default: Default::None,
                version: None,
            }
        );
    }

    #[test]
    fn parse_field_no_subattrs() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[nix()])];
        let ctx = Context::new();
        let field = Field::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            field,
            Field {
                default: Default::None,
                version: None,
            }
        );
    }

    #[test]
    fn parse_variant_version() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[nix(version="..34")])];
        let ctx = Context::new();
        let variant = Variant::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            variant,
            Variant {
                version: parse_quote!(..34),
            }
        );
    }

    #[test]
    fn parse_variant_no_attr() {
        let attrs: Vec<Attribute> = vec![];
        let ctx = Context::new();
        let variant = Variant::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            variant,
            Variant {
                version: parse_quote!(..),
            }
        );
    }

    #[test]
    fn parse_variant_no_subattrs() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[nix()])];
        let ctx = Context::new();
        let variant = Variant::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            variant,
            Variant {
                version: parse_quote!(..),
            }
        );
    }

    #[test]
    fn parse_container_try_from() {
        let attrs: Vec<Attribute> = vec![parse_quote!(#[nix(try_from="u64")])];
        let ctx = Context::new();
        let container = Container::from_ast(&ctx, &attrs);
        ctx.check().unwrap();
        assert_eq!(
            container,
            Container {
                from_str: None,
                type_from: None,
                type_try_from: Some(parse_quote!(u64)),
                crate_path: None,
            }
        );
    }
}
