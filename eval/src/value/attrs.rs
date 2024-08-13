//! This module implements Nix attribute sets. They have flexible
//! backing implementations, as they are used in very versatile
//! use-cases that are all exposed the same way in the language
//! surface.
//!
//! Due to this, construction and management of attribute sets has
//! some peculiarities that are encapsulated within this module.
use std::borrow::Borrow;
use std::collections::{btree_map, BTreeMap};
use std::iter::FromIterator;
use std::rc::Rc;

use bstr::BStr;
use lazy_static::lazy_static;
use serde::de::{Deserializer, Error, Visitor};
use serde::Deserialize;

use super::string::NixString;
use super::thunk::ThunkSet;
use super::TotalDisplay;
use super::Value;
use crate::errors::ErrorKind;
use crate::CatchableErrorKind;

lazy_static! {
    static ref NAME_S: NixString = "name".into();
    static ref NAME_REF: &'static NixString = &NAME_S;
    static ref VALUE_S: NixString = "value".into();
    static ref VALUE_REF: &'static NixString = &VALUE_S;
}

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Deserialize, Default)]
pub(super) enum AttrsRep {
    #[default]
    Empty,

    Map(BTreeMap<NixString, Value>),

    /// Warning: this represents a **two**-attribute attrset, with
    /// attribute names "name" and "value", like `{name="foo";
    /// value="bar";}`, *not* `{foo="bar";}`!
    KV {
        name: Value,
        value: Value,
    },
}

impl AttrsRep {
    fn select(&self, key: &BStr) -> Option<&Value> {
        match self {
            AttrsRep::Empty => None,

            AttrsRep::KV { name, value } => match &**key {
                b"name" => Some(name),
                b"value" => Some(value),
                _ => None,
            },

            AttrsRep::Map(map) => map.get(key),
        }
    }

    fn contains(&self, key: &BStr) -> bool {
        match self {
            AttrsRep::Empty => false,
            AttrsRep::KV { .. } => key == "name" || key == "value",
            AttrsRep::Map(map) => map.contains_key(key),
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Debug, Default)]
pub struct NixAttrs(pub(super) Rc<AttrsRep>);

impl From<AttrsRep> for NixAttrs {
    fn from(rep: AttrsRep) -> Self {
        NixAttrs(Rc::new(rep))
    }
}

impl<K, V> FromIterator<(K, V)> for NixAttrs
where
    NixString: From<K>,
    Value: From<V>,
{
    fn from_iter<T>(iter: T) -> NixAttrs
    where
        T: IntoIterator<Item = (K, V)>,
    {
        AttrsRep::Map(
            iter.into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
        )
        .into()
    }
}

impl From<BTreeMap<NixString, Value>> for NixAttrs {
    fn from(map: BTreeMap<NixString, Value>) -> Self {
        AttrsRep::Map(map).into()
    }
}

impl TotalDisplay for NixAttrs {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        if let Some(Value::String(s)) = self.select("type") {
            if *s == "derivation" {
                write!(f, "«derivation ")?;
                if let Some(p) = self.select("drvPath") {
                    p.total_fmt(f, set)?;
                } else {
                    write!(f, "???")?;
                }
                return write!(f, "»");
            }
        }

        f.write_str("{ ")?;

        for (name, value) in self.iter_sorted() {
            write!(f, "{} = ", name.ident_str())?;
            value.total_fmt(f, set)?;
            f.write_str("; ")?;
        }

        f.write_str("}")
    }
}

impl<'de> Deserialize<'de> for NixAttrs {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MapVisitor;

        impl<'de> Visitor<'de> for MapVisitor {
            type Value = NixAttrs;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid Nix attribute set")
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                let mut stack_array = Vec::with_capacity(map.size_hint().unwrap_or(0) * 2);

                while let Some((key, value)) = map.next_entry()? {
                    stack_array.push(key);
                    stack_array.push(value);
                }

                Ok(NixAttrs::construct(stack_array.len() / 2, stack_array)
                    .map_err(A::Error::custom)?
                    .expect("Catchable values are unreachable here"))
            }
        }

        deserializer.deserialize_map(MapVisitor)
    }
}

impl NixAttrs {
    pub fn empty() -> Self {
        AttrsRep::Empty.into()
    }

    /// Compare two attribute sets by pointer equality. Only makes
    /// sense for some attribute set representations, i.e. returning
    /// `false` does not mean that the two attribute sets do not have
    /// equal *content*.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }

    /// Return an attribute set containing the merge of the two
    /// provided sets. Keys from the `other` set have precedence.
    pub fn update(self, other: Self) -> Self {
        // Short-circuit on some optimal cases:
        match (self.0.as_ref(), other.0.as_ref()) {
            (AttrsRep::Empty, AttrsRep::Empty) => return self,
            (AttrsRep::Empty, _) => return other,
            (_, AttrsRep::Empty) => return self,
            (AttrsRep::KV { .. }, AttrsRep::KV { .. }) => return other,

            // Explicitly handle all branches instead of falling
            // through, to ensure that we get at least some compiler
            // errors if variants are modified.
            (AttrsRep::Map(_), AttrsRep::Map(_))
            | (AttrsRep::Map(_), AttrsRep::KV { .. })
            | (AttrsRep::KV { .. }, AttrsRep::Map(_)) => {}
        };

        // Slightly more advanced, but still optimised updates
        match (Rc::unwrap_or_clone(self.0), Rc::unwrap_or_clone(other.0)) {
            (AttrsRep::Map(mut m), AttrsRep::KV { name, value }) => {
                m.insert(NAME_S.clone(), name);
                m.insert(VALUE_S.clone(), value);
                AttrsRep::Map(m).into()
            }

            (AttrsRep::KV { name, value }, AttrsRep::Map(mut m)) => {
                match m.entry(NAME_S.clone()) {
                    btree_map::Entry::Vacant(e) => {
                        e.insert(name);
                    }

                    btree_map::Entry::Occupied(_) => { /* name from `m` has precedence */ }
                };

                match m.entry(VALUE_S.clone()) {
                    btree_map::Entry::Vacant(e) => {
                        e.insert(value);
                    }

                    btree_map::Entry::Occupied(_) => { /* value from `m` has precedence */ }
                };

                AttrsRep::Map(m).into()
            }

            // Plain merge of maps.
            (AttrsRep::Map(mut m1), AttrsRep::Map(m2)) => {
                m1.extend(m2);
                AttrsRep::Map(m1).into()
            }

            // Cases handled above by the borrowing match:
            _ => unreachable!(),
        }
    }

    /// Return the number of key-value entries in an attrset.
    pub fn len(&self) -> usize {
        match self.0.as_ref() {
            AttrsRep::Map(map) => map.len(),
            AttrsRep::Empty => 0,
            AttrsRep::KV { .. } => 2,
        }
    }

    pub fn is_empty(&self) -> bool {
        match self.0.as_ref() {
            AttrsRep::Map(map) => map.is_empty(),
            AttrsRep::Empty => true,
            AttrsRep::KV { .. } => false,
        }
    }

    /// Select a value from an attribute set by key.
    pub fn select<K>(&self, key: &K) -> Option<&Value>
    where
        K: Borrow<BStr> + ?Sized,
    {
        self.0.select(key.borrow())
    }

    /// Select a required value from an attribute set by key, return
    /// an `AttributeNotFound` error if it is missing.
    pub fn select_required<K>(&self, key: &K) -> Result<&Value, ErrorKind>
    where
        K: Borrow<BStr> + ?Sized,
    {
        self.select(key)
            .ok_or_else(|| ErrorKind::AttributeNotFound {
                name: key.borrow().to_string(),
            })
    }

    pub fn contains<'a, K: 'a>(&self, key: K) -> bool
    where
        &'a BStr: From<K>,
    {
        self.0.contains(key.into())
    }

    /// Construct an iterator over all the key-value pairs in the attribute set.
    #[allow(clippy::needless_lifetimes)]
    pub fn iter<'a>(&'a self) -> Iter<KeyValue<'a>> {
        Iter(match &self.0.as_ref() {
            AttrsRep::Map(map) => KeyValue::Map(map.iter()),
            AttrsRep::Empty => KeyValue::Empty,

            AttrsRep::KV {
                ref name,
                ref value,
            } => KeyValue::KV {
                name,
                value,
                at: IterKV::default(),
            },
        })
    }

    /// Same as iter(), but marks call sites which rely on the
    /// iteration being lexicographic.
    pub fn iter_sorted(&self) -> Iter<KeyValue<'_>> {
        self.iter()
    }

    /// Same as [IntoIterator::into_iter], but marks call sites which rely on the
    /// iteration being lexicographic.
    pub fn into_iter_sorted(self) -> OwnedAttrsIterator {
        self.into_iter()
    }

    /// Construct an iterator over all the keys of the attribute set
    pub fn keys(&self) -> Keys {
        Keys(match self.0.as_ref() {
            AttrsRep::Empty => KeysInner::Empty,
            AttrsRep::KV { .. } => KeysInner::KV(IterKV::default()),

            // TODO(tazjin): only sort when required, not always.
            AttrsRep::Map(m) => KeysInner::Map(m.keys()),
        })
    }

    /// Implement construction logic of an attribute set, to encapsulate
    /// logic about attribute set optimisations inside of this module.
    pub fn construct(
        count: usize,
        mut stack_slice: Vec<Value>,
    ) -> Result<Result<Self, CatchableErrorKind>, ErrorKind> {
        debug_assert!(
            stack_slice.len() == count * 2,
            "construct_attrs called with count == {}, but slice.len() == {}",
            count,
            stack_slice.len(),
        );

        // Optimisation: Empty attribute set
        if count == 0 {
            return Ok(Ok(AttrsRep::Empty.into()));
        }

        // Optimisation: KV pattern
        if count == 2 {
            if let Some(kv) = attempt_optimise_kv(&mut stack_slice) {
                return Ok(Ok(kv));
            }
        }

        let mut attrs_map = BTreeMap::new();

        for _ in 0..count {
            let value = stack_slice.pop().unwrap();
            let key = stack_slice.pop().unwrap();

            match key {
                Value::String(ks) => set_attr(&mut attrs_map, ks, value)?,

                Value::Null => {
                    // This is in fact valid, but leads to the value
                    // being ignored and nothing being set, i.e. `{
                    // ${null} = 1; } => { }`.
                    continue;
                }

                Value::Catchable(err) => return Ok(Err(*err)),

                other => return Err(ErrorKind::InvalidAttributeName(other)),
            }
        }

        Ok(Ok(AttrsRep::Map(attrs_map).into()))
    }

    /// Construct an optimized "KV"-style attribute set given the value for the
    /// `"name"` key, and the value for the `"value"` key
    pub(crate) fn from_kv(name: Value, value: Value) -> Self {
        AttrsRep::KV { name, value }.into()
    }
}

impl IntoIterator for NixAttrs {
    type Item = (NixString, Value);
    type IntoIter = OwnedAttrsIterator;

    fn into_iter(self) -> Self::IntoIter {
        match Rc::unwrap_or_clone(self.0) {
            AttrsRep::Empty => OwnedAttrsIterator(IntoIterRepr::Empty),
            AttrsRep::KV { name, value } => OwnedAttrsIterator(IntoIterRepr::Finite(
                vec![(NAME_REF.clone(), name), (VALUE_REF.clone(), value)].into_iter(),
            )),
            AttrsRep::Map(map) => OwnedAttrsIterator(IntoIterRepr::Map(map.into_iter())),
        }
    }
}

/// In Nix, name/value attribute pairs are frequently constructed from
/// literals. This particular case should avoid allocation of a map,
/// additional heap values etc. and use the optimised `KV` variant
/// instead.
///
/// ```norust
/// `slice` is the top of the stack from which the attrset is being
/// constructed, e.g.
///
///   slice: [ "value" 5 "name" "foo" ]
///   index:   0       1 2      3
///   stack:   3       2 1      0
/// ```
fn attempt_optimise_kv(slice: &mut [Value]) -> Option<NixAttrs> {
    let (name_idx, value_idx) = {
        match (&slice[2], &slice[0]) {
            (Value::String(s1), Value::String(s2)) if (*s1 == *NAME_S && *s2 == *VALUE_S) => (3, 1),
            (Value::String(s1), Value::String(s2)) if (*s1 == *VALUE_S && *s2 == *NAME_S) => (1, 3),

            // Technically this branch lets type errors pass,
            // but they will be caught during normal attribute
            // set construction instead.
            _ => return None,
        }
    };

    Some(NixAttrs::from_kv(
        slice[name_idx].clone(),
        slice[value_idx].clone(),
    ))
}

/// Set an attribute on an in-construction attribute set, while
/// checking against duplicate keys.
fn set_attr(
    map: &mut BTreeMap<NixString, Value>,
    key: NixString,
    value: Value,
) -> Result<(), ErrorKind> {
    match map.entry(key) {
        btree_map::Entry::Occupied(entry) => Err(ErrorKind::DuplicateAttrsKey {
            key: entry.key().to_string(),
        }),

        btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            Ok(())
        }
    }
}

/// Internal helper type to track the iteration status of an iterator
/// over the name/value representation.
#[derive(Debug, Default)]
pub enum IterKV {
    #[default]
    Name,
    Value,
    Done,
}

impl IterKV {
    fn next(&mut self) {
        match *self {
            Self::Name => *self = Self::Value,
            Self::Value => *self = Self::Done,
            Self::Done => {}
        }
    }
}

/// Iterator representation over the keys *and* values of an attribute
/// set.
pub enum KeyValue<'a> {
    Empty,

    KV {
        name: &'a Value,
        value: &'a Value,
        at: IterKV,
    },

    Map(btree_map::Iter<'a, NixString, Value>),
}

/// Iterator over a Nix attribute set.
// This wrapper type exists to make the inner "raw" iterator
// inaccessible.
#[repr(transparent)]
pub struct Iter<T>(T);

impl<'a> Iterator for Iter<KeyValue<'a>> {
    type Item = (&'a NixString, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            KeyValue::Map(inner) => inner.next(),
            KeyValue::Empty => None,

            KeyValue::KV { name, value, at } => match at {
                IterKV::Name => {
                    at.next();
                    Some((&NAME_REF, name))
                }

                IterKV::Value => {
                    at.next();
                    Some((&VALUE_REF, value))
                }

                IterKV::Done => None,
            },
        }
    }
}

impl<'a> ExactSizeIterator for Iter<KeyValue<'a>> {
    fn len(&self) -> usize {
        match &self.0 {
            KeyValue::Empty => 0,
            KeyValue::KV { .. } => 2,
            KeyValue::Map(inner) => inner.len(),
        }
    }
}

enum KeysInner<'a> {
    Empty,
    KV(IterKV),
    Map(btree_map::Keys<'a, NixString, Value>),
}

pub struct Keys<'a>(KeysInner<'a>);

impl<'a> Iterator for Keys<'a> {
    type Item = &'a NixString;

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            KeysInner::Empty => None,
            KeysInner::KV(at @ IterKV::Name) => {
                at.next();
                Some(&NAME_REF)
            }
            KeysInner::KV(at @ IterKV::Value) => {
                at.next();
                Some(&VALUE_REF)
            }
            KeysInner::KV(IterKV::Done) => None,
            KeysInner::Map(m) => m.next(),
        }
    }
}

impl<'a> IntoIterator for &'a NixAttrs {
    type Item = (&'a NixString, &'a Value);

    type IntoIter = Iter<KeyValue<'a>>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> ExactSizeIterator for Keys<'a> {
    fn len(&self) -> usize {
        match &self.0 {
            KeysInner::Empty => 0,
            KeysInner::KV(_) => 2,
            KeysInner::Map(m) => m.len(),
        }
    }
}

/// Internal representation of an owning attrset iterator
pub enum IntoIterRepr {
    Empty,
    Finite(std::vec::IntoIter<(NixString, Value)>),
    Map(btree_map::IntoIter<NixString, Value>),
}

/// Wrapper type which hides the internal implementation details from
/// users.
#[repr(transparent)]
pub struct OwnedAttrsIterator(IntoIterRepr);

impl Iterator for OwnedAttrsIterator {
    type Item = (NixString, Value);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IntoIterRepr::Empty => None,
            IntoIterRepr::Finite(inner) => inner.next(),
            IntoIterRepr::Map(m) => m.next(),
        }
    }
}

impl ExactSizeIterator for OwnedAttrsIterator {
    fn len(&self) -> usize {
        match &self.0 {
            IntoIterRepr::Empty => 0,
            IntoIterRepr::Finite(inner) => inner.len(),
            IntoIterRepr::Map(inner) => inner.len(),
        }
    }
}

impl DoubleEndedIterator for OwnedAttrsIterator {
    fn next_back(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IntoIterRepr::Empty => None,
            IntoIterRepr::Finite(inner) => inner.next_back(),
            IntoIterRepr::Map(inner) => inner.next_back(),
        }
    }
}
