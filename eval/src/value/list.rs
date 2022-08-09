/// This module implements Nix lists.
use std::fmt::Display;

use super::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct NixList(pub Vec<Value>);

impl Display for NixList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // TODO(tazjin): format lists properly
        f.write_fmt(format_args!("<list({})>", self.0.len()))
    }
}
