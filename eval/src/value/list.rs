//! This module implements Nix lists.
use std::ops::Deref;
use std::rc::Rc;

use crate::errors::ErrorKind;
use crate::vm::VM;

use super::thunk::ThunkSet;
use super::TotalDisplay;
use super::Value;

#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct NixList(Rc<Vec<Value>>);

impl TotalDisplay for NixList {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        f.write_str("[ ")?;

        for v in self.0.as_ref() {
            v.total_fmt(f, set)?;
            f.write_str(" ")?;
        }

        f.write_str("]")
    }
}

impl From<Vec<Value>> for NixList {
    fn from(vs: Vec<Value>) -> Self {
        Self(Rc::new(vs))
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
            any_with::<Rc<Vec<Value>>>(args).prop_map(Self).boxed()
        }
    }
}

impl NixList {
    pub fn new() -> Self {
        Self(Rc::new(vec![]))
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

        NixList(Rc::new(stack_slice))
    }

    pub fn iter(&self) -> std::slice::Iter<Value> {
        self.0.iter()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
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

    pub fn into_vec(self) -> Vec<Value> {
        crate::unwrap_or_clone_rc(self.0)
    }
}

impl IntoIterator for NixList {
    type Item = Value;
    type IntoIter = std::vec::IntoIter<Value>;

    fn into_iter(self) -> std::vec::IntoIter<Value> {
        self.into_vec().into_iter()
    }
}

impl<'a> IntoIterator for &'a NixList {
    type Item = &'a Value;

    type IntoIter = std::slice::Iter<'a, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl Deref for NixList {
    type Target = Vec<Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
