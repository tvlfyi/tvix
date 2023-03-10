//! This module implements Nix lists.
use std::ops::Index;

use imbl::{vector, Vector};

use serde::{Deserialize, Serialize};

use crate::errors::AddContext;
use crate::errors::ErrorKind;
use crate::vm::generators;
use crate::vm::generators::GenCo;
use crate::vm::VM;

use super::thunk::ThunkSet;
use super::TotalDisplay;
use super::Value;

#[repr(transparent)]
#[derive(Clone, Debug, Deserialize, Serialize)]
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

impl From<Vector<Value>> for NixList {
    fn from(vs: Vector<Value>) -> Self {
        Self(vs)
    }
}

impl NixList {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn get(&self, i: usize) -> Option<&Value> {
        self.0.get(i)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn construct(count: usize, stack_slice: Vec<Value>) -> Self {
        debug_assert!(
            count == stack_slice.len(),
            "NixList::construct called with count == {}, but slice.len() == {}",
            count,
            stack_slice.len(),
        );

        NixList(Vector::from_iter(stack_slice.into_iter()))
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

    #[deprecated(note = "callers should avoid constructing from Vec")]
    pub fn from_vec(vs: Vec<Value>) -> Self {
        Self(Vector::from_iter(vs.into_iter()))
    }

    /// Asynchronous sorting algorithm in which the comparator can make use of
    /// VM requests (required as `builtins.sort` uses comparators written in
    /// Nix).
    ///
    /// This is a simple, optimised bubble sort implementation. The choice of
    /// algorithm is constrained by the comparator in Nix not being able to
    /// yield equality, and us being unable to use the standard library
    /// implementation of sorting (which is a lot longer, but a lot more
    /// efficient) here.
    // TODO(amjoseph): Investigate potential impl in Nix code, or Tvix bytecode.
    pub async fn sort_by(&mut self, co: &GenCo, cmp: Value) -> Result<(), ErrorKind> {
        let mut len = self.len();

        loop {
            let mut new_len = 0;
            for i in 1..len {
                if generators::request_force(
                    co,
                    generators::request_call_with(
                        co,
                        cmp.clone(),
                        [self.0[i].clone(), self.0[i - 1].clone()],
                    )
                    .await,
                )
                .await
                .as_bool()
                .context("evaluating comparator in `builtins.sort`")?
                {
                    self.0.swap(i, i - 1);
                    new_len = i;
                }
            }

            if new_len == 0 {
                break;
            }

            len = new_len;
        }

        Ok(())
    }
}

impl IntoIterator for NixList {
    type Item = Value;
    type IntoIter = imbl::vector::ConsumingIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a NixList {
    type Item = &'a Value;
    type IntoIter = imbl::vector::Iter<'a, Value>;

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
