use std::fmt::Display;

/// This module implements Nix language strings and their different
/// backing implementations.

#[derive(Debug, Hash, PartialEq)]
pub struct NixString(String);

impl Display for NixString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.as_str())
    }
}
