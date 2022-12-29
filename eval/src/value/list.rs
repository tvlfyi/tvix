//! This module implements Nix lists.
use std::ops::Index;

use im::{vector, Vector};

use crate::errors::ErrorKind;
use crate::vm::VM;

use super::thunk::ThunkSet;
use super::TotalDisplay;
use super::Value;

#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct NixList(Vector<Value>);

impl TotalDisplay for NixList {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        f.write_str("[ ")?;

        for v in self {
            v.total_fmt(f, set)?;
            f.write_str(" ")?;
        }

        f.write_str("]")
    }
}

// TODO(tazjin): uses of this instance are likely inefficient and can be optimised.
// Eventually this instance should be removed.
impl From<Vec<Value>> for NixList {
    fn from(vs: Vec<Value>) -> Self {
        Self(Vector::from_iter(vs.into_iter()))
    }
}

impl From<Vector<Value>> for NixList {
    fn from(vs: Vector<Value>) -> Self {
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
        // TODO(tazjin): im seems to implement arbitrary instances,
        // but I couldn't figure out how to enable them.
        type Parameters = <Vec<Value> as Arbitrary>::Parameters;
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            any_with::<Vec<Value>>(args).prop_map(|v| v.into()).boxed()
        }
    }
}

impl NixList {
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

        stack_slice.into()
    }

    pub fn iter(&self) -> vector::Iter<Value> {
        self.0.iter()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        self.0.ptr_eq(&other.0)
    }

    /// Compare `self` against `other` for equality using Nix equality semantics
    pub fn nix_eq(&self, other: &Self, vm: &mut VM) -> Result<bool, ErrorKind> {
        if self.ptr_eq(other) {
            return Ok(true);
        }
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

    /// force each element of the list (shallowly), making it safe to call .get().value()
    pub fn force_elements(&self, vm: &mut VM) -> Result<(), ErrorKind> {
        self.iter().try_for_each(|v| v.force(vm).map(|_| ()))
    }

    pub fn into_inner(self) -> Vector<Value> {
        self.0
    }
}

impl IntoIterator for NixList {
    type Item = Value;
    type IntoIter = im::vector::ConsumingIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a NixList {
    type Item = &'a Value;
    type IntoIter = im::vector::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Index<usize> for NixList {
    type Output = Value;

    fn index(&self, index: usize) -> &Self::Output {
        &self.0[index]
    }
}
