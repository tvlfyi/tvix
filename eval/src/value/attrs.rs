//! This module implements Nix attribute sets. They have flexible
//! backing implementations, as they are used in very versatile
//! use-cases that are all exposed the same way in the language
//! surface.
//!
//! Due to this, construction and management of attribute sets has
//! some peculiarities that are encapsulated within this module.
use std::iter::FromIterator;

use imbl::{ordmap, OrdMap};
use lazy_static::lazy_static;
use serde::de::{Deserializer, Error, Visitor};
use serde::Deserialize;

use super::string::NixString;
use super::thunk::ThunkSet;
use super::CoercionKind;
use super::TotalDisplay;
use super::Value;
use crate::errors::ErrorKind;
use crate::generators::{self, GenCo};

lazy_static! {
    static ref NAME_S: NixString = "name".into();
    static ref NAME_REF: &'static NixString = &NAME_S;
    static ref VALUE_S: NixString = "value".into();
    static ref VALUE_REF: &'static NixString = &VALUE_S;
}

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Deserialize)]
pub(super) enum AttrsRep {
    Empty,

    Im(OrdMap<NixString, Value>),

    /// Warning: this represents a **two**-attribute attrset, with
    /// attribute names "name" and "value", like `{name="foo";
    /// value="bar";}`, *not* `{foo="bar";}`!
    KV {
        name: Value,
        value: Value,
    },
}

impl Default for AttrsRep {
    fn default() -> Self {
        AttrsRep::Empty
    }
}

impl AttrsRep {
    /// Retrieve reference to a mutable map inside of an attrs,
    /// optionally changing the representation if required.
    fn map_mut(&mut self) -> &mut OrdMap<NixString, Value> {
        match self {
            AttrsRep::Im(m) => m,

            AttrsRep::Empty => {
                *self = AttrsRep::Im(OrdMap::new());
                self.map_mut()
            }

            AttrsRep::KV { name, value } => {
                *self = AttrsRep::Im(ordmap! {
                    NAME_S.clone() => name.clone(),
                    VALUE_S.clone() => value.clone()
                });

                self.map_mut()
            }
        }
    }

    fn select(&self, key: &str) -> Option<&Value> {
        match self {
            AttrsRep::Empty => None,

            AttrsRep::KV { name, value } => match key {
                "name" => Some(name),
                "value" => Some(value),
                _ => None,
            },

            AttrsRep::Im(map) => map.get(&key.into()),
        }
    }

    fn contains(&self, key: &str) -> bool {
        match self {
            AttrsRep::Empty => false,
            AttrsRep::KV { .. } => key == "name" || key == "value",
            AttrsRep::Im(map) => map.contains_key(&key.into()),
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Debug, Default)]
pub struct NixAttrs(pub(super) AttrsRep);

impl From<OrdMap<NixString, Value>> for NixAttrs {
    fn from(map: OrdMap<NixString, Value>) -> Self {
        NixAttrs(AttrsRep::Im(map))
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
        NixAttrs(AttrsRep::Im(iter.into_iter().collect()))
    }
}

impl TotalDisplay for NixAttrs {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        f.write_str("{ ")?;

        match &self.0 {
            AttrsRep::KV { name, value } => {
                f.write_str("name = ")?;
                name.total_fmt(f, set)?;
                f.write_str("; ")?;

                f.write_str("value = ")?;
                value.total_fmt(f, set)?;
                f.write_str("; ")?;
            }

            AttrsRep::Im(map) => {
                for (name, value) in map {
                    write!(f, "{} = ", name.ident_str())?;
                    value.total_fmt(f, set)?;
                    f.write_str("; ")?;
                }
            }

            AttrsRep::Empty => { /* no values to print! */ }
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

                NixAttrs::construct(stack_array.len() / 2, stack_array).map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_map(MapVisitor)
    }
}

impl NixAttrs {
    pub fn empty() -> Self {
        Self(AttrsRep::Empty)
    }

    /// Compare two attribute sets by pointer equality. Only makes
    /// sense for some attribute set reprsentations, i.e. returning
    /// `false` does not mean that the two attribute sets do not have
    /// equal *content*.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (AttrsRep::Im(lhs), AttrsRep::Im(rhs)) => lhs.ptr_eq(rhs),
            _ => false,
        }
    }

    /// Return an attribute set containing the merge of the two
    /// provided sets. Keys from the `other` set have precedence.
    pub fn update(self, other: Self) -> Self {
        // Short-circuit on some optimal cases:
        match (&self.0, &other.0) {
            (AttrsRep::Empty, AttrsRep::Empty) => return self,
            (AttrsRep::Empty, _) => return other,
            (_, AttrsRep::Empty) => return self,
            (AttrsRep::KV { .. }, AttrsRep::KV { .. }) => return other,

            // Explicitly handle all branches instead of falling
            // through, to ensure that we get at least some compiler
            // errors if variants are modified.
            (AttrsRep::Im(_), AttrsRep::Im(_))
            | (AttrsRep::Im(_), AttrsRep::KV { .. })
            | (AttrsRep::KV { .. }, AttrsRep::Im(_)) => {}
        };

        // Slightly more advanced, but still optimised updates
        match (self.0, other.0) {
            (AttrsRep::Im(mut m), AttrsRep::KV { name, value }) => {
                m.insert(NAME_S.clone(), name);
                m.insert(VALUE_S.clone(), value);
                NixAttrs(AttrsRep::Im(m))
            }

            (AttrsRep::KV { name, value }, AttrsRep::Im(mut m)) => {
                match m.entry(NAME_S.clone()) {
                    imbl::ordmap::Entry::Vacant(e) => {
                        e.insert(name);
                    }

                    imbl::ordmap::Entry::Occupied(_) => { /* name from `m` has precedence */ }
                };

                match m.entry(VALUE_S.clone()) {
                    imbl::ordmap::Entry::Vacant(e) => {
                        e.insert(value);
                    }

                    imbl::ordmap::Entry::Occupied(_) => { /* value from `m` has precedence */ }
                };

                NixAttrs(AttrsRep::Im(m))
            }

            // Plain merge of maps.
            (AttrsRep::Im(m1), AttrsRep::Im(m2)) => NixAttrs(AttrsRep::Im(m2.union(m1))),

            // Cases handled above by the borrowing match:
            _ => unreachable!(),
        }
    }

    /// Return the number of key-value entries in an attrset.
    pub fn len(&self) -> usize {
        match &self.0 {
            AttrsRep::Im(map) => map.len(),
            AttrsRep::Empty => 0,
            AttrsRep::KV { .. } => 2,
        }
    }

    pub fn is_empty(&self) -> bool {
        match &self.0 {
            AttrsRep::Im(map) => map.is_empty(),
            AttrsRep::Empty => true,
            AttrsRep::KV { .. } => false,
        }
    }

    /// Select a value from an attribute set by key.
    pub fn select(&self, key: &str) -> Option<&Value> {
        self.0.select(key)
    }

    /// Select a required value from an attribute set by key, return
    /// an `AttributeNotFound` error if it is missing.
    pub fn select_required(&self, key: &str) -> Result<&Value, ErrorKind> {
        self.select(key)
            .ok_or_else(|| ErrorKind::AttributeNotFound { name: key.into() })
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.contains(key)
    }

    /// Construct an iterator over all the key-value pairs in the attribute set.
    #[allow(clippy::needless_lifetimes)]
    pub fn iter<'a>(&'a self) -> Iter<KeyValue<'a>> {
        Iter(match &self.0 {
            AttrsRep::Im(map) => KeyValue::Im(map.iter()),
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

    pub fn into_iter(self) -> IntoIter {
        match self.0 {
            AttrsRep::Empty => IntoIter(IntoIterRepr::Empty),
            AttrsRep::KV { name, value } => IntoIter(IntoIterRepr::Finite(
                vec![(NAME_REF.clone(), name), (VALUE_REF.clone(), value)].into_iter(),
            )),
            AttrsRep::Im(map) => IntoIter(IntoIterRepr::Im(map.into_iter())),
        }
    }

    /// Same as into_iter(), but marks call sites which rely on the
    /// iteration being lexicographic.
    pub fn into_iter_sorted(self) -> IntoIter {
        self.into_iter()
    }

    /// Construct an iterator over all the keys of the attribute set
    pub fn keys(&self) -> Keys {
        Keys(match &self.0 {
            AttrsRep::Empty => KeysInner::Empty,
            AttrsRep::Im(m) => KeysInner::Im(m.keys()),
            AttrsRep::KV { .. } => KeysInner::KV(IterKV::default()),
        })
    }

    /// Implement construction logic of an attribute set, to encapsulate
    /// logic about attribute set optimisations inside of this module.
    pub fn construct(count: usize, mut stack_slice: Vec<Value>) -> Result<Self, ErrorKind> {
        debug_assert!(
            stack_slice.len() == count * 2,
            "construct_attrs called with count == {}, but slice.len() == {}",
            count,
            stack_slice.len(),
        );

        // Optimisation: Empty attribute set
        if count == 0 {
            return Ok(NixAttrs(AttrsRep::Empty));
        }

        // Optimisation: KV pattern
        if count == 2 {
            if let Some(kv) = attempt_optimise_kv(&mut stack_slice) {
                return Ok(kv);
            }
        }

        let mut attrs = NixAttrs(AttrsRep::Im(OrdMap::new()));

        for _ in 0..count {
            let value = stack_slice.pop().unwrap();
            let key = stack_slice.pop().unwrap();

            match key {
                Value::String(ks) => set_attr(&mut attrs, ks, value)?,

                Value::Null => {
                    // This is in fact valid, but leads to the value
                    // being ignored and nothing being set, i.e. `{
                    // ${null} = 1; } => { }`.
                    continue;
                }

                other => return Err(ErrorKind::InvalidAttributeName(other)),
            }
        }

        Ok(attrs)
    }

    /// Construct an optimized "KV"-style attribute set given the value for the
    /// `"name"` key, and the value for the `"value"` key
    pub(crate) fn from_kv(name: Value, value: Value) -> Self {
        NixAttrs(AttrsRep::KV { name, value })
    }

    /// Attempt to coerce an attribute set with a `__toString`
    /// attribute to a string.
    pub(crate) async fn try_to_string(&self, co: &GenCo, kind: CoercionKind) -> Option<NixString> {
        if let Some(to_string) = self.select("__toString") {
            let callable = generators::request_force(&co, to_string.clone()).await;

            // Leave the attribute set on the stack as an argument
            // to the function call.
            generators::request_stack_push(&co, Value::Attrs(Box::new(self.clone()))).await;

            // Call the callable ...
            let result = generators::request_call(&co, callable).await;

            // Recurse on the result, as attribute set coercion
            // actually works recursively, e.g. you can even return
            // /another/ set with a __toString attr.
            let s = generators::request_string_coerce(&co, result, kind).await;

            return Some(s);
        }

        None
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
fn set_attr(attrs: &mut NixAttrs, key: NixString, value: Value) -> Result<(), ErrorKind> {
    match attrs.0.map_mut().entry(key) {
        imbl::ordmap::Entry::Occupied(entry) => Err(ErrorKind::DuplicateAttrsKey {
            key: entry.key().as_str().to_string(),
        }),

        imbl::ordmap::Entry::Vacant(entry) => {
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

    Im(imbl::ordmap::Iter<'a, NixString, Value>),
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
            KeyValue::Im(inner) => inner.next(),
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
            KeyValue::Im(inner) => inner.len(),
        }
    }
}

enum KeysInner<'a> {
    Empty,
    KV(IterKV),
    Im(imbl::ordmap::Keys<'a, NixString, Value>),
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
            KeysInner::Im(m) => m.next(),
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
            KeysInner::Im(m) => m.len(),
        }
    }
}

/// Internal representation of an owning attrset iterator
pub enum IntoIterRepr {
    Empty,
    Finite(std::vec::IntoIter<(NixString, Value)>),
    Im(imbl::ordmap::ConsumingIter<(NixString, Value)>),
}

#[repr(transparent)]
pub struct IntoIter(IntoIterRepr);

impl Iterator for IntoIter {
    type Item = (NixString, Value);

    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            IntoIterRepr::Empty => None,
            IntoIterRepr::Finite(inner) => inner.next(),
            IntoIterRepr::Im(inner) => inner.next(),
        }
    }
}

impl ExactSizeIterator for IntoIter {
    fn len(&self) -> usize {
        match &self.0 {
            IntoIterRepr::Empty => 0,
            IntoIterRepr::Finite(inner) => inner.len(),
            IntoIterRepr::Im(inner) => inner.len(),
        }
    }
}
