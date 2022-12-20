//! This module implements Nix language strings and their different
//! backing implementations.
use rnix::ast;
use smol_str::SmolStr;
use std::ffi::OsStr;
use std::hash::Hash;
use std::ops::Deref;
use std::path::Path;
use std::{borrow::Cow, fmt::Display, str::Chars};

#[derive(Clone, Debug)]
enum StringRepr {
    Smol(SmolStr),
    Heap(String),
}

#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct NixString(StringRepr);

impl PartialEq for NixString {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for NixString {}

impl PartialOrd for NixString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_str().partial_cmp(other.as_str())
    }
}

impl Ord for NixString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl From<&str> for NixString {
    fn from(s: &str) -> Self {
        NixString(StringRepr::Smol(SmolStr::new(s)))
    }
}

impl From<String> for NixString {
    fn from(s: String) -> Self {
        NixString(StringRepr::Heap(s))
    }
}

impl From<SmolStr> for NixString {
    fn from(s: SmolStr) -> Self {
        NixString(StringRepr::Smol(s))
    }
}

impl From<ast::Ident> for NixString {
    fn from(ident: ast::Ident) -> Self {
        ident.ident_token().unwrap().text().into()
    }
}

impl Hash for NixString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

#[cfg(feature = "arbitrary")]
mod arbitrary {
    use super::*;
    use proptest::prelude::{any_with, Arbitrary};
    use proptest::prop_oneof;
    use proptest::strategy::{BoxedStrategy, Strategy};

    impl Arbitrary for NixString {
        type Parameters = <String as Arbitrary>::Parameters;

        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            prop_oneof![
                // Either generate `StringRepr::Heap`...
                any_with::<String>(args).prop_map(Self::from),
                // ...or generate `StringRepr::Smol` (which `impl From<&str> for NixString` returns)
                any_with::<String>(args).prop_map(|s| Self::from(s.as_str())),
            ]
            .boxed()
        }
    }
}

impl NixString {
    pub const NAME: Self = NixString(StringRepr::Smol(SmolStr::new_inline("name")));
    pub const NAME_REF: &'static Self = &Self::NAME;

    pub const VALUE: Self = NixString(StringRepr::Smol(SmolStr::new_inline("value")));
    pub const VALUE_REF: &'static Self = &Self::VALUE;

    pub fn as_str(&self) -> &str {
        match &self.0 {
            StringRepr::Smol(s) => s.as_str(),
            StringRepr::Heap(s) => s,
        }
    }

    /// Return a displayable representation of the string as an
    /// identifier.
    ///
    /// This is used when printing out strings used as e.g. attribute
    /// set keys, as those are only escaped in the presence of special
    /// characters.
    pub fn ident_str(&self) -> Cow<str> {
        let escaped = nix_escape_string(self.as_str());

        match escaped {
            // A borrowed string is unchanged and can be returned as
            // is.
            Cow::Borrowed(_) => {
                if is_valid_nix_identifier(&escaped) {
                    escaped
                } else {
                    Cow::Owned(format!("\"{}\"", escaped))
                }
            }

            // An owned string has escapes, and needs the outer quotes
            // for display.
            Cow::Owned(s) => Cow::Owned(format!("\"{}\"", s)),
        }
    }

    pub fn concat(&self, other: &Self) -> Self {
        let mut s = self.as_str().to_owned();
        s.push_str(other.as_str());
        NixString(StringRepr::Heap(s))
    }

    pub fn chars(&self) -> Chars<'_> {
        match &self.0 {
            StringRepr::Heap(h) => h.chars(),
            StringRepr::Smol(s) => s.chars(),
        }
    }
}

fn nix_escape_char(ch: char, next: Option<&char>) -> Option<&'static str> {
    match (ch, next) {
        ('\\', _) => Some("\\\\"),
        ('"', _) => Some("\\\""),
        ('\n', _) => Some("\\n"),
        ('\t', _) => Some("\\t"),
        ('\r', _) => Some("\\r"),
        ('$', Some('{')) => Some("\\$"),
        _ => None,
    }
}

/// Return true if this string can be used as an identifier in Nix.
fn is_valid_nix_identifier(s: &str) -> bool {
    // adapted from rnix-parser's tokenizer.rs
    let mut chars = s.chars();
    match chars.next() {
        Some('a'..='z' | 'A'..='Z' | '_') => (),
        _ => return false,
    }
    for c in chars {
        match c {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' => (),
            _ => return false,
        }
    }
    true
}

/// Escape a Nix string for display, as most user-visible representation
/// are escaped strings.
///
/// Note that this does not add the outer pair of surrounding quotes.
fn nix_escape_string(input: &str) -> Cow<str> {
    let mut iter = input.chars().enumerate().peekable();

    while let Some((i, c)) = iter.next() {
        if let Some(esc) = nix_escape_char(c, iter.peek().map(|(_, c)| c)) {
            let mut escaped = String::with_capacity(input.len());
            escaped.push_str(&input[..i]);
            escaped.push_str(esc);

            let mut inner_iter = input[i + 1..].chars().peekable();
            while let Some(c) = inner_iter.next() {
                match nix_escape_char(c, inner_iter.peek()) {
                    Some(esc) => escaped.push_str(esc),
                    None => escaped.push(c),
                }
            }

            return Cow::Owned(escaped);
        }
    }

    Cow::Borrowed(input)
}

impl Display for NixString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("\"")?;
        f.write_str(&nix_escape_string(self.as_str()))?;
        f.write_str("\"")
    }
}

impl AsRef<str> for NixString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<OsStr> for NixString {
    fn as_ref(&self) -> &OsStr {
        self.as_str().as_ref()
    }
}

impl AsRef<Path> for NixString {
    fn as_ref(&self) -> &Path {
        self.as_str().as_ref()
    }
}

impl Deref for NixString {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::properties::{eq_laws, hash_laws, ord_laws};

    eq_laws!(NixString);
    hash_laws!(NixString);
    ord_laws!(NixString);
}
