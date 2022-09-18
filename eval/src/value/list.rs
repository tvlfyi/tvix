//! This module implements Nix lists.
use std::fmt::Display;

use crate::errors::ErrorKind;
use crate::vm::VM;

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

impl From<Vec<Value>> for NixList {
    fn from(vs: Vec<Value>) -> Self {
        Self(vs)
    }
}

#[cfg(feature = "arbitrary")]
mod arbitrary {
    use proptest::{
        prelude::{any_with, Arbitrary},
        strategy::{BoxedStrategy, Strategy},
    };

    use super::*;

    impl Arbitrary for NixList {
        type Parameters = <Vec<Value> as Arbitrary>::Parameters;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            any_with::<Vec<Value>>(args).prop_map(Self).boxed()
        }
    }
}

impl NixList {
    pub fn concat(&self, other: &Self) -> Self {
        let mut lhs = self.clone();
        let mut rhs = other.clone();
        lhs.0.append(&mut rhs.0);
        lhs
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, i: usize) -> Option<&Value> {
        self.0.get(i)
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

    pub fn iter(&self) -> std::slice::Iter<Value> {
        self.0.iter()
    }

    pub fn into_iter(self) -> std::vec::IntoIter<Value> {
        self.0.into_iter()
    }

    /// Compare `self` against `other` for equality using Nix equality semantics
    pub fn nix_eq(&self, other: &Self, vm: &mut VM) -> Result<bool, ErrorKind> {
        if self.len() != other.len() {
            return Ok(false);
        }

        for (v1, v2) in self.iter().zip(other.iter()) {
            if !v1.nix_eq(v2, vm)? {
                return Ok(false);
            }
        }

        Ok(true)
    }
}
