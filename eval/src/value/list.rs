//! This module implements Nix lists.
use std::ops::Index;
use std::rc::Rc;

use imbl::{vector, Vector};

use serde::Deserialize;

use crate::generators;
use crate::generators::GenCo;
use crate::AddContext;
use crate::ErrorKind;

use super::thunk::ThunkSet;
use super::TotalDisplay;
use super::Value;

#[repr(transparent)]
#[derive(Clone, Debug, Deserialize)]
pub struct NixList(Rc<Vector<Value>>);

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
        Self(Rc::new(vs))
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

        NixList(Rc::new(Vector::from_iter(stack_slice.into_iter())))
    }

    pub fn iter(&self) -> vector::Iter<Value> {
        self.0.iter()
    }

    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    pub fn into_inner(self) -> Vector<Value> {
        Rc::try_unwrap(self.0).unwrap_or_else(|rc| (*rc).clone())
    }

    #[deprecated(note = "callers should avoid constructing from Vec")]
    pub fn from_vec(vs: Vec<Value>) -> Self {
        Self(Rc::new(Vector::from_iter(vs.into_iter())))
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
    pub async fn sort_by(self, co: &GenCo, cmp: Value) -> Result<Self, ErrorKind> {
        let mut len = self.len();
        let mut data = self.into_inner();

        loop {
            let mut new_len = 0;
            for i in 1..len {
                if generators::request_force(
                    co,
                    generators::request_call_with(
                        co,
                        cmp.clone(),
                        [data[i].clone(), data[i - 1].clone()],
                    )
                    .await,
                )
                .await
                .as_bool()
                .context("evaluating comparator in `builtins.sort`")?
                {
                    data.swap(i, i - 1);
                    new_len = i;
                }
            }

            if new_len == 0 {
                break;
            }

            len = new_len;
        }

        Ok(data.into())
    }
}

impl IntoIterator for NixList {
    type Item = Value;
    type IntoIter = imbl::vector::ConsumingIter<Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.into_inner().into_iter()
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
