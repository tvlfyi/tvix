use std::{borrow::Cow, fmt::Display};

/// This module implements Nix language strings and their different
/// backing implementations.

#[derive(Clone, Debug, Hash, Eq, Ord)]
pub enum NixString {
    Static(&'static str),
    Heap(String),
}

impl PartialEq for NixString {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialOrd for NixString {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.as_str().partial_cmp(other.as_str())
    }
}

impl From<&'static str> for NixString {
    fn from(s: &'static str) -> Self {
        NixString::Static(s)
    }
}

impl From<String> for NixString {
    fn from(s: String) -> Self {
        NixString::Heap(s)
    }
}

impl NixString {
    pub const NAME: Self = NixString::Static("name");
    pub const VALUE: Self = NixString::Static("value");

    pub fn as_str(&self) -> &str {
        match self {
            NixString::Static(s) => s,
            NixString::Heap(s) => s,
        }
    }
}

fn nix_escape_char(ch: char) -> Option<&'static str> {
    match ch {
        '\\' => Some("\\"),
        '"' => Some("\\"),
        '\n' => Some("\\n"),
        _ => None,
    }
}

// Escape a Nix string for display, as the user-visible representation
// is always an escaped string (except for traces).
//
// Note that this does not add the outer pair of surrounding quotes.
fn escape_string(input: &str) -> Cow<str> {
    for (i, c) in input.chars().enumerate() {
        if let Some(esc) = nix_escape_char(c) {
            let mut escaped = String::with_capacity(input.len());
            escaped.push_str(&input[..i]);
            escaped.push_str(esc);

            for c in input[i + 1..].chars() {
                match nix_escape_char(c) {
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
        match self {
            NixString::Static(s) => f.write_str(&escape_string(s))?,
            NixString::Heap(s) => f.write_str(&escape_string(s))?,
        };
        f.write_str("\"")
    }
}
