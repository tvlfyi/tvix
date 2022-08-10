use std::fmt::Display;

/// This module implements Nix language strings and their different
/// backing implementations.

#[derive(Clone, Debug, Hash, Eq, Ord)]
pub enum NixString {
    Static(&'static str),
    Heap(String),
}

impl Display for NixString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NixString::Static(s) => f.write_str(s),
            NixString::Heap(s) => f.write_str(s),
        }
    }
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
