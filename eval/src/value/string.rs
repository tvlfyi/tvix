//! This module implements Nix language strings.
//!
//! Nix language strings never need to be modified on the language
//! level, allowing us to shave off some memory overhead and only
//! paying the cost when creating new strings.
use bstr::{BStr, BString, ByteSlice, Chars};
use rnix::ast;
use std::borrow::{Borrow, Cow};
use std::collections::HashSet;
use std::fmt::Display;
use std::hash::Hash;
use std::ops::Deref;

use serde::de::{Deserializer, Visitor};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Hash, PartialEq, Eq)]
pub enum NixContextElement {
    /// A plain store path (e.g. source files copied to the store)
    Plain(String),

    /// Single output of a derivation, represented by its name and its derivation path.
    Single { name: String, derivation: String },

    /// A reference to a complete derivation
    /// including its source and its binary closure.
    /// It is used for the `drvPath` attribute context.
    /// The referred string is the store path to
    /// the derivation path.
    Derivation(String),
}

/// Nix context strings representation in Tvix. This tracks a set of different kinds of string
/// dependencies that we can come across during manipulation of our language primitives, mostly
/// strings. There's some simple algebra of context strings and how they propagate w.r.t. primitive
/// operations, e.g. concatenation, interpolation and other string operations.
#[repr(transparent)]
#[derive(Clone, Debug, Serialize, Default)]
pub struct NixContext(HashSet<NixContextElement>);

impl From<NixContextElement> for NixContext {
    fn from(value: NixContextElement) -> Self {
        Self([value].into())
    }
}

impl NixContext {
    /// Creates an empty context that can be populated
    /// and passed to form a contextful [NixString], albeit
    /// if the context is concretly empty, the resulting [NixString]
    /// will be contextless.
    pub fn new() -> Self {
        Self::default()
    }

    /// For internal consumers, we let people observe
    /// if the [NixContext] is actually empty or not
    /// to decide whether they want to skip the allocation
    /// of a full blown [HashSet].
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes a new [NixContextElement] and add it if not already
    /// present in this context.
    pub fn append(mut self, other: NixContextElement) -> Self {
        self.0.insert(other);
        self
    }

    /// Consumes both ends of the join into a new NixContent
    /// containing the union of elements of both ends.
    pub fn join(mut self, other: &mut NixContext) -> Self {
        let other_set = std::mem::take(&mut other.0);
        let mut set: HashSet<NixContextElement> = std::mem::take(&mut self.0);
        set.extend(other_set);
        Self(set)
    }

    /// Copies from another [NixString] its context strings
    /// in this context.
    pub fn mimic(&mut self, other: &NixString) {
        if let Some(ref context) = other.1 {
            self.0.extend(context.iter().cloned());
        }
    }

    /// Iterates over "plain" context elements, e.g. sources imported
    /// in the store without more information, i.e. `toFile` or coerced imported paths.
    /// It yields paths to the store.
    pub fn iter_plain(&self) -> impl Iterator<Item = &str> {
        self.iter().filter_map(|elt| {
            if let NixContextElement::Plain(s) = elt {
                Some(s.as_str())
            } else {
                None
            }
        })
    }

    /// Iterates over "full derivations" context elements, e.g. something
    /// referring to their `drvPath`, i.e. their full sources and binary closure.
    /// It yields derivation paths.
    pub fn iter_derivation(&self) -> impl Iterator<Item = &str> {
        self.iter().filter_map(|elt| {
            if let NixContextElement::Derivation(s) = elt {
                Some(s.as_str())
            } else {
                None
            }
        })
    }

    /// Iterates over "single" context elements, e.g. single derived paths,
    /// or also known as the single output of a given derivation.
    /// The first element of the tuple is the output name
    /// and the second element is the derivation path.
    pub fn iter_single_outputs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.iter().filter_map(|elt| {
            if let NixContextElement::Single { name, derivation } = elt {
                Some((name.as_str(), derivation.as_str()))
            } else {
                None
            }
        })
    }

    /// Iterates over any element of the context.
    pub fn iter(&self) -> impl Iterator<Item = &NixContextElement> {
        self.0.iter()
    }

    /// Produces a list of owned references to this current context,
    /// no matter its type.
    pub fn to_owned_references(self) -> Vec<String> {
        self.0
            .into_iter()
            .map(|ctx| match ctx {
                NixContextElement::Derivation(drv_path) => drv_path,
                NixContextElement::Plain(store_path) => store_path,
                NixContextElement::Single { derivation, .. } => derivation,
            })
            .collect()
    }
}

// FIXME: when serializing, ignore the context?
#[derive(Clone, Debug, Serialize)]
pub struct NixString(Box<BStr>, Option<NixContext>);

impl PartialEq for NixString {
    fn eq(&self, other: &Self) -> bool {
        self.as_bstr() == other.as_bstr()
    }
}

impl Eq for NixString {}

impl PartialEq<&[u8]> for NixString {
    fn eq(&self, other: &&[u8]) -> bool {
        **self == **other
    }
}

impl PartialEq<&str> for NixString {
    fn eq(&self, other: &&str) -> bool {
        **self == other.as_bytes()
    }
}

impl PartialOrd for NixString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for NixString {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.as_bstr().cmp(other.as_bstr())
    }
}

impl From<Box<BStr>> for NixString {
    fn from(value: Box<BStr>) -> Self {
        Self(value, None)
    }
}

impl From<BString> for NixString {
    fn from(value: BString) -> Self {
        Self(Vec::<u8>::from(value).into_boxed_slice().into(), None)
    }
}

impl From<&BStr> for NixString {
    fn from(value: &BStr) -> Self {
        value.to_owned().into()
    }
}

impl From<&[u8]> for NixString {
    fn from(value: &[u8]) -> Self {
        Self::from(value.to_owned())
    }
}

impl From<Vec<u8>> for NixString {
    fn from(value: Vec<u8>) -> Self {
        value.into_boxed_slice().into()
    }
}

impl From<Box<[u8]>> for NixString {
    fn from(value: Box<[u8]>) -> Self {
        Self(value.into(), None)
    }
}

impl From<&str> for NixString {
    fn from(s: &str) -> Self {
        s.as_bytes().into()
    }
}

impl From<String> for NixString {
    fn from(s: String) -> Self {
        s.into_bytes().into()
    }
}

impl<T> From<(T, Option<NixContext>)> for NixString
where
    NixString: From<T>,
{
    fn from((s, ctx): (T, Option<NixContext>)) -> Self {
        NixString(NixString::from(s).0, ctx)
    }
}

impl From<Box<str>> for NixString {
    fn from(s: Box<str>) -> Self {
        s.into_boxed_bytes().into()
    }
}

impl From<ast::Ident> for NixString {
    fn from(ident: ast::Ident) -> Self {
        ident.ident_token().unwrap().text().into()
    }
}

impl<'a> From<&'a NixString> for &'a BStr {
    fn from(s: &'a NixString) -> Self {
        BStr::new(&*s.0)
    }
}

impl From<NixString> for Box<BStr> {
    fn from(s: NixString) -> Self {
        s.0
    }
}

impl From<NixString> for BString {
    fn from(s: NixString) -> Self {
        s.0.to_vec().into()
    }
}

impl AsRef<[u8]> for NixString {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Borrow<BStr> for NixString {
    fn borrow(&self) -> &BStr {
        self.as_bstr()
    }
}

impl Hash for NixString {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_bstr().hash(state)
    }
}

impl<'de> Deserialize<'de> for NixString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StringVisitor;

        impl<'de> Visitor<'de> for StringVisitor {
            type Value = NixString;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid Nix string")
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.into())
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(v.into())
            }
        }

        deserializer.deserialize_string(StringVisitor)
    }
}

impl Deref for NixString {
    type Target = BStr;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(feature = "arbitrary")]
mod arbitrary {
    use super::*;
    use proptest::prelude::{any_with, Arbitrary};
    use proptest::strategy::{BoxedStrategy, Strategy};

    impl Arbitrary for NixString {
        type Parameters = <String as Arbitrary>::Parameters;

        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            any_with::<String>(args).prop_map(Self::from).boxed()
        }
    }
}

impl NixString {
    pub fn new_inherit_context_from<T>(other: &NixString, new_contents: T) -> Self
    where
        NixString: From<T>,
    {
        Self(Self::from(new_contents).0, other.1.clone())
    }

    pub fn new_context_from<T>(context: NixContext, contents: T) -> Self
    where
        NixString: From<T>,
    {
        Self(
            Self::from(contents).0,
            if context.is_empty() {
                None
            } else {
                Some(context)
            },
        )
    }

    pub fn as_bstr(&self) -> &BStr {
        self
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn into_bstring(self) -> BString {
        (*self.0).to_owned()
    }

    /// Return a displayable representation of the string as an
    /// identifier.
    ///
    /// This is used when printing out strings used as e.g. attribute
    /// set keys, as those are only escaped in the presence of special
    /// characters.
    pub fn ident_str(&self) -> Cow<str> {
        let escaped = match self.to_str_lossy() {
            Cow::Borrowed(s) => nix_escape_string(s),
            Cow::Owned(s) => nix_escape_string(&s).into_owned().into(),
        };
        match escaped {
            // A borrowed string is unchanged and can be returned as
            // is.
            Cow::Borrowed(_) => {
                if is_valid_nix_identifier(&escaped) && !is_keyword(&escaped) {
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
        let mut s = self.to_vec();
        s.extend(&(***other));

        let context = [&self.1, &other.1]
            .into_iter()
            .flatten()
            .fold(NixContext::new(), |acc_ctx, new_ctx| {
                acc_ctx.join(&mut new_ctx.clone())
            });
        Self::new_context_from(context, s)
    }

    pub(crate) fn context_mut(&mut self) -> Option<&mut NixContext> {
        return self.1.as_mut();
    }

    pub fn iter_context(&self) -> impl Iterator<Item = &NixContext> {
        return self.1.iter();
    }

    pub fn iter_plain(&self) -> impl Iterator<Item = &str> {
        return self.1.iter().flat_map(|context| context.iter_plain());
    }

    pub fn iter_derivation(&self) -> impl Iterator<Item = &str> {
        return self.1.iter().flat_map(|context| context.iter_derivation());
    }

    pub fn iter_single_outputs(&self) -> impl Iterator<Item = (&str, &str)> {
        return self
            .1
            .iter()
            .flat_map(|context| context.iter_single_outputs());
    }

    /// Returns whether this Nix string possess a context or not.
    pub fn has_context(&self) -> bool {
        self.1.is_some()
    }

    /// This clears the context of that string, losing
    /// all dependency tracking information.
    pub fn clear_context(&mut self) {
        self.1 = None;
    }

    pub fn chars(&self) -> Chars<'_> {
        self.0.chars()
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

/// Return true if this string is a keyword -- character strings
/// which lexically match the "identifier" production but are not
/// parsed as identifiers.  See also cppnix commit
/// b72bc4a972fe568744d98b89d63adcd504cb586c.
fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "then" | "else" | "assert" | "with" | "let" | "in" | "rec" | "inherit"
    )
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
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' | '-' | '\'' => (),
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
    let mut iter = input.char_indices().peekable();

    while let Some((i, c)) = iter.next() {
        if let Some(esc) = nix_escape_char(c, iter.peek().map(|(_, c)| c)) {
            let mut escaped = String::with_capacity(input.len());
            escaped.push_str(&input[..i]);
            escaped.push_str(esc);

            // In theory we calculate how many bytes it takes to represent `esc`
            // in UTF-8 and use that for the offset. It is, however, safe to
            // assume that to be 1, as all characters that can be escaped in a
            // Nix string are ASCII.
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
        f.write_str(&nix_escape_string(&self.to_str_lossy()))?;
        f.write_str("\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::properties::{eq_laws, hash_laws, ord_laws};

    #[test]
    fn size() {
        assert_eq!(std::mem::size_of::<NixString>(), 64);
    }

    eq_laws!(NixString);
    hash_laws!(NixString);
    ord_laws!(NixString);
}
