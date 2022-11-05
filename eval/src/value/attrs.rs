//! This module implements Nix attribute sets. They have flexible
//! backing implementations, as they are used in very versatile
//! use-cases that are all exposed the same way in the language
//! surface.
//!
//! Due to this, construction and management of attribute sets has
//! some peculiarities that are encapsulated within this module.
use std::collections::btree_map;
use std::collections::BTreeMap;

use crate::errors::ErrorKind;
use crate::vm::VM;

use super::string::NixString;
use super::thunk::ThunkSet;
use super::TotalDisplay;
use super::Value;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
enum AttrsRep {
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
    /// Retrieve reference to a mutable map inside of an attrs,
    /// optionally changing the representation if required.
    fn map_mut(&mut self) -> &mut BTreeMap<NixString, Value> {
        match self {
            AttrsRep::Map(m) => m,

            AttrsRep::Empty => {
                *self = AttrsRep::Map(BTreeMap::new());
                self.map_mut()
            }

            AttrsRep::KV { name, value } => {
                *self = AttrsRep::Map(BTreeMap::from([
                    (NixString::NAME, name.clone()),
                    (NixString::VALUE, value.clone()),
                ]));
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

            AttrsRep::Map(map) => map.get(&key.into()),
        }
    }

    fn contains(&self, key: &str) -> bool {
        match self {
            AttrsRep::Empty => false,
            AttrsRep::KV { .. } => key == "name" || key == "value",
            AttrsRep::Map(map) => map.contains_key(&key.into()),
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct NixAttrs(AttrsRep);

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

            AttrsRep::Map(map) => {
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

#[cfg(feature = "arbitrary")]
mod arbitrary {
    use super::*;

    use proptest::prelude::*;
    use proptest::prop_oneof;
    use proptest::strategy::{BoxedStrategy, Just, Strategy};

    impl Arbitrary for NixAttrs {
        type Parameters = <BTreeMap<NixString, Value> as Arbitrary>::Parameters;

        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(args: Self::Parameters) -> Self::Strategy {
            prop_oneof![
                Just(Self(AttrsRep::Empty)),
                (
                    any_with::<Value>(args.2.clone()),
                    any_with::<Value>(args.2.clone())
                )
                    .prop_map(|(name, value)| Self(AttrsRep::KV { name, value })),
                any_with::<BTreeMap<NixString, Value>>(args)
                    .prop_map(|map| Self(AttrsRep::Map(map)))
            ]
            .boxed()
        }
    }
}

impl NixAttrs {
    pub fn empty() -> Self {
        Self(AttrsRep::Empty)
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
            (AttrsRep::Map(_), AttrsRep::Map(_))
            | (AttrsRep::Map(_), AttrsRep::KV { .. })
            | (AttrsRep::KV { .. }, AttrsRep::Map(_)) => {}
        };

        // Slightly more advanced, but still optimised updates
        match (self.0, other.0) {
            (AttrsRep::Map(mut m), AttrsRep::KV { name, value }) => {
                m.insert(NixString::NAME, name);
                m.insert(NixString::VALUE, value);
                NixAttrs(AttrsRep::Map(m))
            }

            (AttrsRep::KV { name, value }, AttrsRep::Map(mut m)) => {
                match m.entry(NixString::NAME) {
                    btree_map::Entry::Vacant(e) => {
                        e.insert(name);
                    }

                    btree_map::Entry::Occupied(_) => { /* name from `m` has precedence */ }
                };

                match m.entry(NixString::VALUE) {
                    btree_map::Entry::Vacant(e) => {
                        e.insert(value);
                    }

                    btree_map::Entry::Occupied(_) => { /* value from `m` has precedence */ }
                };

                NixAttrs(AttrsRep::Map(m))
            }

            // Plain merge of maps.
            (AttrsRep::Map(mut m1), AttrsRep::Map(mut m2)) => {
                m1.append(&mut m2);
                NixAttrs(AttrsRep::Map(m1))
            }

            // Cases handled above by the borrowing match:
            _ => unreachable!(),
        }
    }

    /// Return the number of key-value entries in an attrset.
    pub fn len(&self) -> usize {
        match &self.0 {
            AttrsRep::Map(map) => map.len(),
            AttrsRep::Empty => 0,
            AttrsRep::KV { .. } => 2,
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

    /// Construct an iterator over all the keys of the attribute set
    pub fn keys(&self) -> Keys {
        Keys(match &self.0 {
            AttrsRep::Empty => KeysInner::Empty,
            AttrsRep::Map(m) => KeysInner::Map(m.keys()),
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

        // TODO(tazjin): extend_reserve(count) (rust#72631)
        let mut attrs = NixAttrs(AttrsRep::Map(BTreeMap::new()));

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

    /// Construct an attribute set directly from a BTreeMap
    /// representation. This is only visible inside of the crate, as
    /// it is intended exclusively for use with the construction of
    /// global sets for the compiler.
    pub(crate) fn from_map(map: BTreeMap<NixString, Value>) -> Self {
        NixAttrs(AttrsRep::Map(map))
    }

    /// Construct an optimized "KV"-style attribute set given the value for the
    /// `"name"` key, and the value for the `"value"` key
    pub(crate) fn from_kv(name: Value, value: Value) -> Self {
        NixAttrs(AttrsRep::KV { name, value })
    }

    /// Compare `self` against `other` for equality using Nix equality semantics
    pub fn nix_eq(&self, other: &Self, vm: &mut VM) -> Result<bool, ErrorKind> {
        match (&self.0, &other.0) {
            (AttrsRep::Empty, AttrsRep::Empty) => Ok(true),

            // It is possible to create an empty attribute set that
            // has Map representation like so: ` { ${null} = 1; }`.
            //
            // Preventing this would incur a cost on all attribute set
            // construction (we'd have to check the actual number of
            // elements after key construction). In practice this
            // probably does not happen, so it's better to just bite
            // the bullet and implement this branch.
            (AttrsRep::Empty, AttrsRep::Map(map)) | (AttrsRep::Map(map), AttrsRep::Empty) => {
                Ok(map.is_empty())
            }

            // Other specialised representations (KV ...) definitely
            // do not match `Empty`.
            (AttrsRep::Empty, _) | (_, AttrsRep::Empty) => Ok(false),

            (
                AttrsRep::KV {
                    name: n1,
                    value: v1,
                },
                AttrsRep::KV {
                    name: n2,
                    value: v2,
                },
            ) => Ok(n1.nix_eq(n2, vm)? && v1.nix_eq(v2, vm)?),

            (AttrsRep::Map(map), AttrsRep::KV { name, value })
            | (AttrsRep::KV { name, value }, AttrsRep::Map(map)) => {
                if map.len() != 2 {
                    return Ok(false);
                }

                if let (Some(m_name), Some(m_value)) =
                    (map.get(&NixString::NAME), map.get(&NixString::VALUE))
                {
                    return Ok(name.nix_eq(m_name, vm)? && value.nix_eq(m_value, vm)?);
                }

                Ok(false)
            }

            (AttrsRep::Map(m1), AttrsRep::Map(m2)) => {
                if m1.len() != m2.len() {
                    return Ok(false);
                }

                for (k, v1) in m1 {
                    if let Some(v2) = m2.get(k) {
                        if !v1.nix_eq(v2, vm)? {
                            return Ok(false);
                        }
                    } else {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
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
            (Value::String(s1), Value::String(s2))
                if (*s1 == NixString::NAME && *s2 == NixString::VALUE) =>
            {
                (3, 1)
            }

            (Value::String(s1), Value::String(s2))
                if (*s1 == NixString::VALUE && *s2 == NixString::NAME) =>
            {
                (1, 3)
            }

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
        btree_map::Entry::Occupied(entry) => Err(ErrorKind::DuplicateAttrsKey {
            key: entry.key().as_str().to_string(),
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
#[derive(Debug)]
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
                    Some((NixString::NAME_REF, name))
                }

                IterKV::Value => {
                    at.next();
                    Some((NixString::VALUE_REF, value))
                }

                IterKV::Done => None,
            },
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
                Some(NixString::NAME_REF)
            }
            KeysInner::KV(at @ IterKV::Value) => {
                at.next();
                Some(NixString::VALUE_REF)
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
