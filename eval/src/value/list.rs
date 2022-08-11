/// This module implements Nix lists.
use std::fmt::Display;

use super::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct NixList(pub Vec<Value>);

impl Display for NixList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[ ")?;

        for v in &self.0 {
            v.fmt(f)?;
            f.write_str(" ")?;
        }

        f.write_str("]")
    }
}

impl NixList {
    pub fn concat(&self, other: &Self) -> Self {
        let mut lhs = self.clone();
        let mut rhs = other.clone();
        lhs.0.append(&mut rhs.0);
        lhs
    }
}
