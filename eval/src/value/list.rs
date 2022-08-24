//! This module implements Nix lists.
use std::fmt::Display;

use super::Value;

#[repr(transparent)]
#[derive(Clone, Debug, PartialEq)]
pub struct NixList(Vec<Value>);

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

    pub fn construct(count: usize, stack_slice: Vec<Value>) -> Self {
        debug_assert!(
            count == stack_slice.len(),
            "NixList::construct called with count == {}, but slice.len() == {}",
            count,
            stack_slice.len(),
        );

        NixList(stack_slice)
    }

    pub fn into_iter(self) -> std::vec::IntoIter<Value> {
        self.0.into_iter()
    }
}
