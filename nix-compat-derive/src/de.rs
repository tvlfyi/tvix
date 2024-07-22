use proc_macro2::{Span, TokenStream};
use quote::{quote, quote_spanned, ToTokens};
use syn::spanned::Spanned;
use syn::{DeriveInput, Generics, Path, Type};

use crate::internal::attrs::Default;
use crate::internal::inputs::RemoteInput;
use crate::internal::{attrs, Container, Context, Data, Field, Remote, Style, Variant};

pub fn expand_nix_deserialize(nnixrs: Path, input: &mut DeriveInput) -> syn::Result<TokenStream> {
    let cx = Context::new();
    let cont = Container::from_ast(&cx, nnixrs, input);
    cx.check()?;
    let cont = cont.unwrap();

    let ty = cont.ident_type();
    let body = nix_deserialize_body(&cont);
    let crate_path = cont.crate_path();

    Ok(nix_deserialize_impl(
        crate_path,
        &ty,
        &cont.original.generics,
        body,
    ))
}

pub fn expand_nix_deserialize_remote(
    crate_path: Path,
    input: &RemoteInput,
) -> syn::Result<TokenStream> {
    let cx = Context::new();
    let remote = Remote::from_ast(&cx, crate_path, input);
    cx.check()?;
    let remote = remote.unwrap();

    let crate_path = remote.crate_path();
    let body = nix_deserialize_body_from(crate_path, &remote.attrs).expect("From tokenstream");
    let generics = Generics::default();
    Ok(nix_deserialize_impl(crate_path, remote.ty, &generics, body))
}

fn nix_deserialize_impl(
    crate_path: &Path,
    ty: &Type,
    generics: &Generics,
    body: TokenStream,
) -> TokenStream {
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    quote! {
        #[automatically_derived]
        impl #impl_generics #crate_path::nix_daemon::de::NixDeserialize for #ty #ty_generics
            #where_clause
        {
            #[allow(clippy::manual_async_fn)]
            fn try_deserialize<R>(reader: &mut R) -> impl ::std::future::Future<Output=Result<Option<Self>, R::Error>> + Send + '_
                where R: ?Sized + #crate_path::nix_daemon::de::NixRead + Send,
            {
                #body
            }
        }
    }
}

fn nix_deserialize_body_from(
    crate_path: &syn::Path,
    attrs: &attrs::Container,
) -> Option<TokenStream> {
    if let Some(span) = attrs.from_str.as_ref() {
        Some(nix_deserialize_from_str(crate_path, span.span()))
    } else if let Some(type_from) = attrs.type_from.as_ref() {
        Some(nix_deserialize_from(type_from))
    } else {
        attrs
            .type_try_from
            .as_ref()
            .map(|type_try_from| nix_deserialize_try_from(crate_path, type_try_from))
    }
}

fn nix_deserialize_body(cont: &Container) -> TokenStream {
    if let Some(tokens) = nix_deserialize_body_from(cont.crate_path(), &cont.attrs) {
        tokens
    } else {
        match &cont.data {
            Data::Struct(style, fields) => nix_deserialize_struct(*style, fields),
            Data::Enum(variants) => nix_deserialize_enum(variants),
        }
    }
}

fn nix_deserialize_struct(style: Style, fields: &[Field<'_>]) -> TokenStream {
    let read_fields = fields.iter().map(|f| {
        let field = f.var_ident();
        let ty = f.ty;
        let read_value = quote_spanned! {
            ty.span()=> if first__ {
                first__ = false;
                if let Some(v) = reader.try_read_value::<#ty>().await? {
                    v
                } else {
                    return Ok(None);
                }
            } else {
                reader.read_value::<#ty>().await?
            }
        };
        if let Some(version) = f.attrs.version.as_ref() {
            let default = match &f.attrs.default {
                Default::Default => quote_spanned!(ty.span()=>::std::default::Default::default),
                Default::Path(path) => path.to_token_stream(),
                _ => panic!("No default for versioned field"),
            };
            quote! {
                let #field : #ty = if (#version).contains(&reader.version().minor()) {
                    #read_value
                } else {
                    #default()
                };
            }
        } else {
            quote! {
                let #field : #ty = #read_value;
            }
        }
    });

    let field_names = fields.iter().map(|f| f.var_ident());
    let construct = match style {
        Style::Struct => {
            quote! {
                Self { #(#field_names),* }
            }
        }
        Style::Tuple => {
            quote! {
                Self(#(#field_names),*)
            }
        }
        Style::Unit => quote!(Self),
    };
    quote! {
        #[allow(unused_assignments)]
        async move {
            let mut first__ = true;
            #(#read_fields)*
            Ok(Some(#construct))
        }
    }
}

fn nix_deserialize_variant(variant: &Variant<'_>) -> TokenStream {
    let ident = variant.ident;
    let read_fields = variant.fields.iter().map(|f| {
        let field = f.var_ident();
        let ty = f.ty;
        let read_value = quote_spanned! {
            ty.span()=> if first__ {
                first__ = false;
                if let Some(v) = reader.try_read_value::<#ty>().await? {
                    v
                } else {
                    return Ok(None);
                }
            } else {
                reader.read_value::<#ty>().await?
            }
        };
        if let Some(version) = f.attrs.version.as_ref() {
            let default = match &f.attrs.default {
                Default::Default => quote_spanned!(ty.span()=>::std::default::Default::default),
                Default::Path(path) => path.to_token_stream(),
                _ => panic!("No default for versioned field"),
            };
            quote! {
                let #field : #ty = if (#version).contains(&reader.version().minor()) {
                    #read_value
                } else {
                    #default()
                };
            }
        } else {
            quote! {
                let #field : #ty = #read_value;
            }
        }
    });
    let field_names = variant.fields.iter().map(|f| f.var_ident());
    let construct = match variant.style {
        Style::Struct => {
            quote! {
                Self::#ident { #(#field_names),* }
            }
        }
        Style::Tuple => {
            quote! {
                Self::#ident(#(#field_names),*)
            }
        }
        Style::Unit => quote!(Self::#ident),
    };
    let version = &variant.attrs.version;
    quote! {
        #version => {
            #(#read_fields)*
            Ok(Some(#construct))
        }
    }
}

fn nix_deserialize_enum(variants: &[Variant<'_>]) -> TokenStream {
    let match_variant = variants
        .iter()
        .map(|variant| nix_deserialize_variant(variant));
    quote! {
        #[allow(unused_assignments)]
        async move {
            let mut first__ = true;
            match reader.version().minor() {
                #(#match_variant)*
            }
        }
    }
}

fn nix_deserialize_from(ty: &Type) -> TokenStream {
    quote_spanned! {
        ty.span() =>
        async move {
            if let Some(value) = reader.try_read_value::<#ty>().await? {
                Ok(Some(<Self as ::std::convert::From<#ty>>::from(value)))
            } else {
                Ok(None)
            }
        }
    }
}

fn nix_deserialize_try_from(crate_path: &Path, ty: &Type) -> TokenStream {
    quote_spanned! {
        ty.span() =>
        async move {
            use #crate_path::nix_daemon::de::Error;
            if let Some(item) = reader.try_read_value::<#ty>().await? {
                <Self as ::std::convert::TryFrom<#ty>>::try_from(item)
                    .map_err(Error::invalid_data)
                    .map(Some)
            } else {
                Ok(None)
            }
        }
    }
}

fn nix_deserialize_from_str(crate_path: &Path, span: Span) -> TokenStream {
    quote_spanned! {
        span =>
        async move {
            use #crate_path::nix_daemon::de::Error;
            if let Some(buf) = reader.try_read_bytes().await? {
                let s = ::std::str::from_utf8(&buf)
                    .map_err(Error::invalid_data)?;
                <Self as ::std::str::FromStr>::from_str(s)
                    .map_err(Error::invalid_data)
                    .map(Some)
            } else {
                Ok(None)
            }
        }
    }
}
