#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteInput {
    pub attrs: Vec<syn::Attribute>,
    pub ident: syn::Type,
}

impl syn::parse::Parse for RemoteInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let attrs = input.call(syn::Attribute::parse_outer)?;

        let ident = input.parse::<syn::Type>()?;
        Ok(RemoteInput { attrs, ident })
    }
}

impl quote::ToTokens for RemoteInput {
    fn to_tokens(&self, tokens: &mut proc_macro2::TokenStream) {
        fn is_outer(attr: &&syn::Attribute) -> bool {
            match attr.style {
                syn::AttrStyle::Outer => true,
                syn::AttrStyle::Inner(_) => false,
            }
        }
        for attr in self.attrs.iter().filter(is_outer) {
            attr.to_tokens(tokens);
        }
        self.ident.to_tokens(tokens);
    }
}

#[cfg(test)]
mod test {
    use syn::parse_quote;
    //use syn::parse::Parse;

    use super::*;

    #[test]
    fn test_input() {
        let p: RemoteInput = parse_quote!(u64);
        assert_eq!(
            p,
            RemoteInput {
                attrs: vec![],
                ident: parse_quote!(u64),
            }
        );
    }

    #[test]
    fn test_input_attr() {
        let p: RemoteInput = parse_quote!(
            #[nix]
            u64
        );
        assert_eq!(
            p,
            RemoteInput {
                attrs: vec![parse_quote!(#[nix])],
                ident: parse_quote!(u64),
            }
        );
    }

    #[test]
    fn test_input_attr_multiple() {
        let p: RemoteInput = parse_quote!(
            #[nix]
            #[hello]
            u64
        );
        assert_eq!(
            p,
            RemoteInput {
                attrs: vec![parse_quote!(#[nix]), parse_quote!(#[hello])],
                ident: parse_quote!(u64),
            }
        );
    }

    #[test]
    fn test_input_attr_full() {
        let p: RemoteInput = parse_quote!(
            #[nix(try_from = "u64")]
            usize
        );
        assert_eq!(
            p,
            RemoteInput {
                attrs: vec![parse_quote!(#[nix(try_from="u64")])],
                ident: parse_quote!(usize),
            }
        );
    }

    #[test]
    fn test_input_attr_other() {
        let p: RemoteInput = parse_quote!(
            #[muh]
            u64
        );
        assert_eq!(
            p,
            RemoteInput {
                attrs: vec![parse_quote!(#[muh])],
                ident: parse_quote!(u64),
            }
        );
    }
}
