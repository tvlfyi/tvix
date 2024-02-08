//! This module implements Nix lists.
use std::ops::Index;
use std::rc::Rc;

use imbl::{vector, Vector};

use serde::Deserialize;

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

        NixList(Rc::new(Vector::from_iter(stack_slice)))
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
        Self(Rc::new(Vector::from_iter(vs)))
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
