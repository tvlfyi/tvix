/// This module implements Nix attribute sets. They have flexible
/// backing implementations, as they are used in very versatile
/// use-cases that are all exposed the same way in the language
/// surface.
///
/// Due to this, construction and management of attribute sets has
/// some peculiarities that are encapsulated within this module.
use std::collections::BTreeMap;
use std::fmt::Display;
use std::rc::Rc;

use crate::errors::{Error, EvalResult};

use super::string::NixString;
use super::Value;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug)]
enum AttrsRep {
    Empty,
    Map(BTreeMap<NixString, Value>),
    KV { name: Value, value: Value },
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
                    (NixString::NAME, std::mem::replace(name, Value::Blackhole)),
                    (NixString::VALUE, std::mem::replace(value, Value::Blackhole)),
                ]));
                self.map_mut()
            }
        }
    }

    fn select(&self, key: &str) -> Option<&Value> {
        match self {
            AttrsRep::Empty => None,

            AttrsRep::KV { name, value } => {
                if key == "name" {
                    return Some(&name);
                }

                if key == "value" {
                    return Some(&value);
                }

                None
            }

            AttrsRep::Map(map) => map.get(&key.into()),
        }
    }
}

#[repr(transparent)]
#[derive(Clone, Debug)]
pub struct NixAttrs(AttrsRep);

impl Display for NixAttrs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("{ ")?;

        match &self.0 {
            AttrsRep::KV { name, value } => {
                f.write_fmt(format_args!("name = {}; ", name))?;
                f.write_fmt(format_args!("value = {}; ", value))?;
            }

            AttrsRep::Map(map) => {
                for (name, value) in map.iter() {
                    f.write_fmt(format_args!("{} = {}; ", name.ident_str(), value))?;
                }
            }

            AttrsRep::Empty => { /* no values to print! */ }
        }

        f.write_str("}")
    }
}

impl PartialEq for NixAttrs {
    fn eq(&self, _other: &Self) -> bool {
        todo!("attrset equality")
    }
}

impl NixAttrs {
    // Update one attribute set with the values of the other.
    pub fn update(&self, other: &Self) -> Self {
        match (&self.0, &other.0) {
            // Short-circuit on some optimal cases:
            (AttrsRep::Empty, AttrsRep::Empty) => NixAttrs(AttrsRep::Empty),
            (AttrsRep::Empty, _) => other.clone(),
            (_, AttrsRep::Empty) => self.clone(),
            (AttrsRep::KV { .. }, AttrsRep::KV { .. }) => other.clone(),

            // Slightly more advanced, but still optimised updates
            (AttrsRep::Map(m), AttrsRep::KV { name, value }) => {
                let mut m = m.clone();
                m.insert(NixString::NAME, name.clone());
                m.insert(NixString::VALUE, value.clone());
                NixAttrs(AttrsRep::Map(m))
            }

            (AttrsRep::KV { name, value }, AttrsRep::Map(m)) => {
                let mut m = m.clone();

                match m.entry(NixString::NAME) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(name.clone());
                    }

                    std::collections::btree_map::Entry::Occupied(_) => {
                        /* name from `m` has precedence */
                    }
                };

                match m.entry(NixString::VALUE) {
                    std::collections::btree_map::Entry::Vacant(e) => {
                        e.insert(value.clone());
                    }

                    std::collections::btree_map::Entry::Occupied(_) => {
                        /* value from `m` has precedence */
                    }
                };

                NixAttrs(AttrsRep::Map(m))
            }

            // Plain merge of maps.
            (AttrsRep::Map(m1), AttrsRep::Map(m2)) => {
                let mut m1 = m1.clone();
                let mut m2 = m2.clone();
                m1.append(&mut m2);
                NixAttrs(AttrsRep::Map(m1))
            }
        }
    }

    // Select a value from an attribute set by key.
    pub fn select(&self, key: &str) -> Option<&Value> {
        self.0.select(key)
    }

    /// Implement construction logic of an attribute set, to encapsulate
    /// logic about attribute set optimisations inside of this module.
    pub fn construct(count: usize, mut stack_slice: Vec<Value>) -> EvalResult<Self> {
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

            // It is at this point that nested attribute sets need to
            // be constructed (if they exist).
            //
            let key = stack_slice.pop().unwrap();
            match key {
                Value::String(ks) => set_attr(&mut attrs, ks, value)?,

                Value::AttrPath(mut path) => {
                    set_nested_attr(
                        &mut attrs,
                        path.pop().expect("AttrPath is never empty"),
                        path,
                        value,
                    )?;
                }

                other => {
                    return Err(Error::InvalidKeyType {
                        given: other.type_of(),
                    })
                }
            }
        }

        Ok(attrs)
    }
}

// In Nix, name/value attribute pairs are frequently constructed from
// literals. This particular case should avoid allocation of a map,
// additional heap values etc. and use the optimised `KV` variant
// instead.
//
// `slice` is the top of the stack from which the attrset is being
// constructed, e.g.
//
//   slice: [ "value" 5 "name" "foo" ]
//   index:   0       1 2      3
//   stack:   3       2 1      0
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

    Some(NixAttrs(AttrsRep::KV {
        name: std::mem::replace(&mut slice[name_idx], Value::Blackhole),
        value: std::mem::replace(&mut slice[value_idx], Value::Blackhole),
    }))
}

// Set an attribute on an in-construction attribute set, while
// checking against duplicate keys.
fn set_attr(attrs: &mut NixAttrs, key: NixString, value: Value) -> EvalResult<()> {
    let attrs = attrs.0.map_mut();
    let entry = attrs.entry(key);

    match entry {
        std::collections::btree_map::Entry::Occupied(entry) => {
            return Err(Error::DuplicateAttrsKey {
                key: entry.key().as_str().to_string(),
            })
        }

        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(value);
            return Ok(());
        }
    };
}

// Set a nested attribute inside of an attribute set, throwing a
// duplicate key error if a non-hashmap entry already exists on the
// path.
//
// There is some optimisation potential for this simple implementation
// if it becomes a problem.
fn set_nested_attr(
    attrs: &mut NixAttrs,
    key: NixString,
    mut path: Vec<NixString>,
    value: Value,
) -> EvalResult<()> {
    // If there is no next key we are at the point where we
    // should insert the value itself.
    if path.is_empty() {
        return set_attr(attrs, key, value);
    }

    let attrs = attrs.0.map_mut();
    let entry = attrs.entry(key);

    // If there is not we go one step further down, in which case we
    // need to ensure that there either is no entry, or the existing
    // entry is a hashmap into which to insert the next value.
    //
    // If a value of a different type exists, the user specified a
    // duplicate key.
    match entry {
        // Vacant entry -> new attribute set is needed.
        std::collections::btree_map::Entry::Vacant(entry) => {
            let mut map = NixAttrs(AttrsRep::Map(BTreeMap::new()));

            // TODO(tazjin): technically recursing further is not
            // required, we can create the whole hierarchy here, but
            // it's noisy.
            set_nested_attr(&mut map, path.pop().expect("next key exists"), path, value)?;

            entry.insert(Value::Attrs(Rc::new(map)));
        }

        // Occupied entry: Either error out if there is something
        // other than attrs, or insert the next value.
        std::collections::btree_map::Entry::Occupied(mut entry) => match entry.get_mut() {
            Value::Attrs(attrs) => {
                set_nested_attr(
                    Rc::make_mut(attrs),
                    path.pop().expect("next key exists"),
                    path,
                    value,
                )?;
            }

            _ => {
                return Err(Error::DuplicateAttrsKey {
                    key: entry.key().as_str().to_string(),
                })
            }
        },
    }

    Ok(())
}
