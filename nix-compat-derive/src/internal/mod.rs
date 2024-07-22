use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::Token;

pub mod attrs;
mod ctx;
pub mod inputs;
mod symbol;

pub use ctx::Context;

pub struct Field<'a> {
    pub member: syn::Member,
    pub ty: &'a syn::Type,
    pub attrs: attrs::Field,
    pub original: &'a syn::Field,
}

impl<'a> Field<'a> {
    pub fn from_ast(ctx: &Context, idx: usize, field: &'a syn::Field) -> Field<'a> {
        let attrs = attrs::Field::from_ast(ctx, &field.attrs);
        let member = match &field.ident {
            Some(id) => syn::Member::Named(id.clone()),
            None => syn::Member::Unnamed(idx.into()),
        };
        Field {
            member,
            attrs,
            ty: &field.ty,
            original: field,
        }
    }

    pub fn var_ident(&self) -> syn::Ident {
        match &self.member {
            syn::Member::Named(name) => name.clone(),
            syn::Member::Unnamed(idx) => {
                syn::Ident::new(&format!("field{}", idx.index), self.original.span())
            }
        }
    }
}

pub struct Variant<'a> {
    pub ident: &'a syn::Ident,
    pub attrs: attrs::Variant,
    pub style: Style,
    pub fields: Vec<Field<'a>>,
    //pub original: &'a syn::Variant,
}

impl<'a> Variant<'a> {
    pub fn from_ast(ctx: &Context, variant: &'a syn::Variant) -> Self {
        let attrs = attrs::Variant::from_ast(ctx, &variant.attrs);
        let (style, fields) = match &variant.fields {
            syn::Fields::Named(fields) => (Style::Struct, fields_ast(ctx, &fields.named)),
            syn::Fields::Unnamed(fields) => (Style::Tuple, fields_ast(ctx, &fields.unnamed)),
            syn::Fields::Unit => (Style::Unit, Vec::new()),
        };
        Variant {
            ident: &variant.ident,
            attrs,
            style,
            fields,
            //original: variant,
        }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum Style {
    Struct,
    Tuple,
    Unit,
}

pub enum Data<'a> {
    Enum(Vec<Variant<'a>>),
    Struct(Style, Vec<Field<'a>>),
}

pub struct Container<'a> {
    pub ident: &'a syn::Ident,
    pub attrs: attrs::Container,
    pub data: Data<'a>,
    pub crate_path: syn::Path,
    pub original: &'a syn::DeriveInput,
}

impl<'a> Container<'a> {
    pub fn from_ast(
        ctx: &Context,
        crate_path: syn::Path,
        input: &'a mut syn::DeriveInput,
    ) -> Option<Container<'a>> {
        let attrs = attrs::Container::from_ast(ctx, &input.attrs);
        let data = match &input.data {
            syn::Data::Struct(s) => match &s.fields {
                syn::Fields::Named(fields) => {
                    Data::Struct(Style::Struct, fields_ast(ctx, &fields.named))
                }
                syn::Fields::Unnamed(fields) => {
                    Data::Struct(Style::Tuple, fields_ast(ctx, &fields.unnamed))
                }
                syn::Fields::Unit => Data::Struct(Style::Unit, Vec::new()),
            },
            syn::Data::Enum(e) => {
                let variants = e
                    .variants
                    .iter()
                    .map(|variant| Variant::from_ast(ctx, variant))
                    .collect();
                Data::Enum(variants)
            }
            syn::Data::Union(u) => {
                ctx.error_spanned(u.union_token, "Union not supported by nixrs");
                return None;
            }
        };
        Some(Container {
            ident: &input.ident,
            attrs,
            data,
            crate_path,
            original: input,
        })
    }

    pub fn crate_path(&self) -> &syn::Path {
        if let Some(crate_path) = self.attrs.crate_path.as_ref() {
            crate_path
        } else {
            &self.crate_path
        }
    }

    pub fn ident_type(&self) -> syn::Type {
        let path: syn::Path = self.ident.clone().into();
        let tp = syn::TypePath { qself: None, path };
        tp.into()
    }
}

pub struct Remote<'a> {
    pub attrs: attrs::Container,
    pub ty: &'a syn::Type,
    pub crate_path: syn::Path,
}

impl<'a> Remote<'a> {
    pub fn from_ast(
        ctx: &Context,
        crate_path: syn::Path,
        input: &'a inputs::RemoteInput,
    ) -> Option<Remote<'a>> {
        let attrs = attrs::Container::from_ast(ctx, &input.attrs);
        if attrs.from_str.is_none() && attrs.type_from.is_none() && attrs.type_try_from.is_none() {
            ctx.error_spanned(input, "Missing from_str, from or try_from attribute");
            return None;
        }
        Some(Remote {
            ty: &input.ident,
            attrs,
            crate_path,
        })
    }

    pub fn crate_path(&self) -> &syn::Path {
        if let Some(crate_path) = self.attrs.crate_path.as_ref() {
            crate_path
        } else {
            &self.crate_path
        }
    }
}

fn fields_ast<'a>(ctx: &Context, fields: &'a Punctuated<syn::Field, Token![,]>) -> Vec<Field<'a>> {
    fields
        .iter()
        .enumerate()
        .map(|(idx, field)| Field::from_ast(ctx, idx, field))
        .collect()
}
