//! This module implements the backing representation of runtime
//! values in the Nix language.
use std::cell::Ref;
use std::ops::Deref;
use std::rc::Rc;
use std::{fmt::Display, path::PathBuf};

#[cfg(feature = "arbitrary")]
mod arbitrary;
mod attrs;
mod builtin;
mod function;
mod list;
mod string;
mod thunk;

use crate::errors::ErrorKind;
use crate::opcode::StackIdx;
use crate::upvalues::UpvalueCarrier;
use crate::vm::VM;
pub use attrs::NixAttrs;
pub use builtin::Builtin;
pub use function::{Closure, Lambda};
pub use list::NixList;
pub use string::NixString;
pub use thunk::Thunk;

#[warn(variant_size_differences)]
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(NixString),
    Path(PathBuf),
    Attrs(Rc<NixAttrs>),
    List(NixList),
    Closure(Closure),
    Builtin(Builtin),

    // Internal values that, while they technically exist at runtime,
    // are never returned to or created directly by users.
    Thunk(Thunk),
    AttrPath(Vec<NixString>),
    AttrNotFound,
    DynamicUpvalueMissing(NixString),
    Blueprint(Rc<Lambda>),
    DeferredUpvalue(StackIdx),
}

// Helper macros to generate the to_*/as_* macros while accounting for
// thunks.

/// Generate an `as_*` method returning a reference to the expected
/// type, or a type error. This only works for types that implement
/// `Copy`, as returning a reference to an inner thunk value is not
/// possible.

/// Generate an `as_*/to_*` accessor method that returns either the
/// expected type, or a type error.
macro_rules! gen_cast {
    ( $name:ident, $type:ty, $expected:expr, $variant:pat, $result:expr ) => {
        pub fn $name(&self) -> Result<$type, ErrorKind> {
            match self {
                $variant => Ok($result),
                Value::Thunk(thunk) => Self::$name(&thunk.value()),
                other => Err(type_error($expected, &other)),
            }
        }
    };
}

/// Generate an `is_*` type-checking method.
macro_rules! gen_is {
    ( $name:ident, $variant:pat ) => {
        pub fn $name(&self) -> bool {
            match self {
                $variant => true,
                Value::Thunk(thunk) => Self::$name(&thunk.value()),
                _ => false,
            }
        }
    };
}

/// Describes what input types are allowed when coercing a `Value` to a string
#[derive(Clone, Copy, Debug)]
pub enum CoercionKind {
    /// Only coerce already "stringly" types like strings and paths, but also
    /// coerce sets that have a `__toString` attribute. Equivalent to
    /// `!coerceMore` in C++ Nix.
    Weak,
    /// Coerce all value types included by `Weak`, but also coerce `null`,
    /// booleans, integers, floats and lists of coercible types. Equivalent to
    /// `coerceMore` in C++ Nix.
    Strong,
}

/// A reference to a [`Value`] returned by a call to [`Value::force`], whether the value was
/// originally a thunk or not.
///
/// Implements [`Deref`] to [`Value`], so can generally be used as a [`Value`]
pub(crate) enum ForceResult<'a> {
    ForcedThunk(Ref<'a, Value>),
    Immediate(&'a Value),
}

impl<'a> Deref for ForceResult<'a> {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        match self {
            ForceResult::ForcedThunk(r) => r,
            ForceResult::Immediate(v) => v,
        }
    }
}

impl Value {
    /// Coerce a `Value` to a string. See `CoercionKind` for a rundown of what
    /// input types are accepted under what circumstances.
    pub fn coerce_to_string(
        &self,
        kind: CoercionKind,
        vm: &mut VM,
    ) -> Result<NixString, ErrorKind> {
        // TODO: eventually, this will need to handle string context and importing
        // files into the Nix store depending on what context the coercion happens in
        if let Value::Thunk(t) = self {
            t.force(vm)?;
        }

        match (self, kind) {
            // deal with thunks
            (Value::Thunk(t), _) => t.value().coerce_to_string(kind, vm),

            // coercions that are always done
            (Value::String(s), _) => Ok(s.clone()),
            // TODO(sterni): Think about proper encoding handling here. This needs
            // general consideration anyways, since one current discrepancy between
            // C++ Nix and Tvix is that the former's strings are arbitrary byte
            // sequences without NUL bytes, whereas Tvix only allows valid
            // Unicode. See also b/189.
            (Value::Path(p), _) => Ok(p.to_string_lossy().into_owned().into()),

            // Attribute sets can be converted to strings if they either have an
            // `__toString` attribute which holds a function that receives the
            // set itself or an `outPath` attribute which should be a string.
            // `__toString` is preferred.
            (Value::Attrs(attrs), _) => {
                match (attrs.select("__toString"), attrs.select("outPath")) {
                    (None, None) => Err(ErrorKind::NotCoercibleToString { from: "set", kind }),

                    (Some(f), _) => {
                        // use a closure here to deal with the thunk borrow we need to do below
                        let call_to_string = |value: &Value, vm: &mut VM| {
                            // TODO(sterni): calling logic should be extracted into a helper
                            let result = match value {
                                Value::Closure(c) => {
                                    vm.push(self.clone());
                                    vm.call(c.lambda(), c.upvalues().clone(), 1)
                                        .map_err(|e| e.kind)
                                }

                                Value::Builtin(b) => {
                                    vm.push(self.clone());
                                    vm.call_builtin(b.clone()).map_err(|e| e.kind)?;
                                    Ok(vm.pop())
                                }

                                _ => Err(ErrorKind::NotCallable),
                            }?;

                            match result {
                                Value::String(s) => Ok(s),
                                // Attribute set coercion actually works
                                // recursively, e.g. you can even return
                                // /another/ set with a __toString attr.
                                _ => result.coerce_to_string(kind, vm),
                            }
                        };

                        if let Value::Thunk(t) = f {
                            t.force(vm)?;
                            let guard = t.value();
                            call_to_string(&*guard, vm)
                        } else {
                            call_to_string(f, vm)
                        }
                    }

                    // Similarly to `__toString` we also coerce recursively for `outPath`
                    (None, Some(s)) => s.coerce_to_string(kind, vm),
                }
            }

            // strong coercions
            (Value::Null, CoercionKind::Strong) | (Value::Bool(false), CoercionKind::Strong) => {
                Ok("".into())
            }
            (Value::Bool(true), CoercionKind::Strong) => Ok("1".into()),

            (Value::Integer(i), CoercionKind::Strong) => Ok(format!("{i}").into()),
            (Value::Float(f), CoercionKind::Strong) => {
                // contrary to normal Display, coercing a float to a string will
                // result in unconditional 6 decimal places
                Ok(format!("{:.6}", f).into())
            }

            // Lists are coerced by coercing their elements and interspersing spaces
            (Value::List(l), CoercionKind::Strong) => {
                // TODO(sterni): use intersperse when it becomes available?
                // https://github.com/rust-lang/rust/issues/79524
                l.iter()
                    .map(|v| v.coerce_to_string(kind, vm))
                    .reduce(|acc, string| {
                        let a = acc?;
                        let s = &string?;
                        Ok(a.concat(&" ".into()).concat(s))
                    })
                    // None from reduce indicates empty iterator
                    .unwrap_or_else(|| Ok("".into()))
            }

            (Value::Closure(_), _)
            | (Value::Builtin(_), _)
            | (Value::Null, _)
            | (Value::Bool(_), _)
            | (Value::Integer(_), _)
            | (Value::Float(_), _)
            | (Value::List(_), _) => Err(ErrorKind::NotCoercibleToString {
                from: self.type_of(),
                kind,
            }),

            (Value::AttrPath(_), _)
            | (Value::AttrNotFound, _)
            | (Value::DynamicUpvalueMissing(_), _)
            | (Value::Blueprint(_), _)
            | (Value::DeferredUpvalue(_), _) => {
                panic!("tvix bug: .coerce_to_string() called on internal value")
            }
        }
    }

    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Integer(_) => "int",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Path(_) => "path",
            Value::Attrs(_) => "set",
            Value::List(_) => "list",
            Value::Closure(_) | Value::Builtin(_) => "lambda",

            // Internal types
            Value::Thunk(_)
            | Value::AttrPath(_)
            | Value::AttrNotFound
            | Value::DynamicUpvalueMissing(_)
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_) => "internal",
        }
    }

    gen_cast!(as_bool, bool, "bool", Value::Bool(b), *b);
    gen_cast!(as_int, i64, "int", Value::Integer(x), *x);
    gen_cast!(to_str, NixString, "string", Value::String(s), s.clone());
    gen_cast!(to_attrs, Rc<NixAttrs>, "set", Value::Attrs(a), a.clone());
    gen_cast!(to_list, NixList, "list", Value::List(l), l.clone());
    gen_cast!(to_closure, Closure, "lambda", Value::Closure(c), c.clone());

    gen_is!(is_number, Value::Integer(_) | Value::Float(_));
    gen_is!(is_bool, Value::Bool(_));

    /// Compare `self` against `other` for equality using Nix equality semantics.
    ///
    /// Takes a reference to the `VM` to allow forcing thunks during comparison
    pub fn nix_eq(&self, other: &Self, vm: &mut VM) -> Result<bool, ErrorKind> {
        match (self, other) {
            // Trivial comparisons
            (Value::Null, Value::Null) => Ok(true),
            (Value::Bool(b1), Value::Bool(b2)) => Ok(b1 == b2),
            (Value::List(l1), Value::List(l2)) => l1.nix_eq(l2, vm),
            (Value::String(s1), Value::String(s2)) => Ok(s1 == s2),
            (Value::Path(p1), Value::Path(p2)) => Ok(p1 == p2),

            // Numerical comparisons (they work between float & int)
            (Value::Integer(i1), Value::Integer(i2)) => Ok(i1 == i2),
            (Value::Integer(i), Value::Float(f)) => Ok(*i as f64 == *f),
            (Value::Float(f1), Value::Float(f2)) => Ok(f1 == f2),
            (Value::Float(f), Value::Integer(i)) => Ok(*i as f64 == *f),

            // Optimised attribute set comparison
            (Value::Attrs(a1), Value::Attrs(a2)) => Ok(Rc::ptr_eq(a1, a2) || a1.nix_eq(a2, vm)?),

            // If either value is a thunk, the thunk should be forced, and then the resulting value
            // must be compared instead.
            (Value::Thunk(lhs), Value::Thunk(rhs)) => {
                lhs.force(vm)?;
                rhs.force(vm)?;

                Ok(*lhs.value() == *rhs.value())
            }
            (Value::Thunk(lhs), rhs) => Ok(&*lhs.value() == rhs),
            (lhs, Value::Thunk(rhs)) => Ok(lhs == &*rhs.value()),

            // Everything else is either incomparable (e.g. internal
            // types) or false.
            // TODO(tazjin): mirror Lambda equality behaviour
            _ => Ok(false),
        }
    }

    /// Ensure `self` is forced if it is a thunk, and return a reference to the resulting value.
    pub(crate) fn force(&self, vm: &mut VM) -> Result<ForceResult, ErrorKind> {
        match self {
            Self::Thunk(thunk) => {
                thunk.force(vm)?;
                Ok(ForceResult::ForcedThunk(thunk.value()))
            }
            _ => Ok(ForceResult::Immediate(self)),
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(true) => f.write_str("true"),
            Value::Bool(false) => f.write_str("false"),
            Value::Integer(num) => write!(f, "{}", num),
            Value::String(s) => s.fmt(f),
            Value::Path(p) => p.display().fmt(f),
            Value::Attrs(attrs) => attrs.fmt(f),
            Value::List(list) => list.fmt(f),
            Value::Closure(_) => f.write_str("lambda"), // TODO: print position
            Value::Builtin(builtin) => builtin.fmt(f),

            // Nix prints floats with a maximum precision of 5 digits
            // only.
            Value::Float(num) => {
                write!(f, "{}", format!("{:.5}", num).trim_end_matches(['.', '0']))
            }

            // Delegate thunk display to the type, as it must handle
            // the case of already evaluated thunks.
            Value::Thunk(t) => t.fmt(f),

            // internal types
            Value::AttrPath(path) => write!(f, "internal[attrpath({})]", path.len()),
            Value::AttrNotFound => f.write_str("internal[not found]"),
            Value::Blueprint(_) => f.write_str("internal[blueprint]"),
            Value::DeferredUpvalue(_) => f.write_str("internal[deferred_upvalue]"),
            Value::DynamicUpvalueMissing(name) => {
                write!(f, "internal[no_dyn_upvalue({name})]")
            }
        }
    }
}

fn type_error(expected: &'static str, actual: &Value) -> ErrorKind {
    ErrorKind::TypeError {
        expected,
        actual: actual.type_of(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {}

    mod nix_eq {
        use crate::observer::NoOpObserver;

        use super::*;
        use proptest::prelude::ProptestConfig;
        use test_strategy::proptest;

        #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
        fn reflexive(x: Value) {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(&mut observer);

            assert!(x.nix_eq(&x, &mut vm).unwrap())
        }

        #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
        fn symmetric(x: Value, y: Value) {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(&mut observer);

            assert_eq!(
                x.nix_eq(&y, &mut vm).unwrap(),
                y.nix_eq(&x, &mut vm).unwrap()
            )
        }

        #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
        fn transitive(x: Value, y: Value, z: Value) {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(&mut observer);

            if x.nix_eq(&y, &mut vm).unwrap() && y.nix_eq(&z, &mut vm).unwrap() {
                assert!(x.nix_eq(&z, &mut vm).unwrap())
            }
        }

        #[test]
        fn list_int_float_fungibility() {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(&mut observer);

            let v1 = Value::List(NixList::from(vec![Value::Integer(1)]));
            let v2 = Value::List(NixList::from(vec![Value::Float(1.0)]));

            assert!(v1.nix_eq(&v2, &mut vm).unwrap())
        }
    }
}
