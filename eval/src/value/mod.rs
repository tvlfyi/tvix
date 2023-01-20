//! This module implements the backing representation of runtime
//! values in the Nix language.
use std::cmp::Ordering;
use std::ops::Deref;
use std::path::PathBuf;
use std::rc::Rc;
use std::{cell::Ref, fmt::Display};

use serde::{Deserialize, Serialize};

#[cfg(feature = "arbitrary")]
mod arbitrary;
mod attrs;
mod builtin;
mod function;
mod list;
mod path;
mod string;
mod thunk;

use crate::errors::ErrorKind;
use crate::opcode::StackIdx;
use crate::vm::VM;
pub use attrs::NixAttrs;
pub use builtin::{Builtin, BuiltinArgument};
pub(crate) use function::Formals;
pub use function::{Closure, Lambda};
pub use list::NixList;
pub use path::canon_path;
pub use string::NixString;
pub use thunk::Thunk;

use self::thunk::ThunkSet;

#[warn(variant_size_differences)]
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(NixString),

    #[serde(skip)]
    Path(PathBuf),
    Attrs(Box<NixAttrs>),
    List(NixList),

    #[serde(skip)]
    Closure(Rc<Closure>), // must use Rc<Closure> here in order to get proper pointer equality

    #[serde(skip)]
    Builtin(Builtin),

    // Internal values that, while they technically exist at runtime,
    // are never returned to or created directly by users.
    #[serde(skip_deserializing)]
    Thunk(Thunk),

    // See [`compiler::compile_select_or()`] for explanation
    #[serde(skip)]
    AttrNotFound,

    // this can only occur in Chunk::Constants and nowhere else
    #[serde(skip)]
    Blueprint(Rc<Lambda>),

    #[serde(skip)]
    DeferredUpvalue(StackIdx),
    #[serde(skip)]
    UnresolvedPath(PathBuf),
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

/// Generate an `as_*_mut/to_*_mut` accessor method that returns either the
/// expected type, or a type error.
macro_rules! gen_cast_mut {
    ( $name:ident, $type:ty, $expected:expr, $variant:ident) => {
        pub fn $name(&mut self) -> Result<&mut $type, ErrorKind> {
            match self {
                Value::$variant(x) => Ok(x),
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
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CoercionKind {
    /// Force thunks, but perform no other coercions.
    ThunksOnly,
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
pub enum ForceResult<'a> {
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

impl<T> From<T> for Value
where
    T: Into<NixString>,
{
    fn from(t: T) -> Self {
        Self::String(t.into())
    }
}

/// Constructors
impl Value {
    /// Construct a [`Value::Attrs`] from a [`NixAttrs`].
    pub fn attrs(attrs: NixAttrs) -> Self {
        Self::Attrs(Box::new(attrs))
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
            (Value::Path(p), kind) if kind != CoercionKind::ThunksOnly => {
                let imported = vm.io().import_path(p)?;
                Ok(imported.to_string_lossy().into_owned().into())
            }

            // Attribute sets can be converted to strings if they either have an
            // `__toString` attribute which holds a function that receives the
            // set itself or an `outPath` attribute which should be a string.
            // `__toString` is preferred.
            (Value::Attrs(attrs), kind) if kind != CoercionKind::ThunksOnly => {
                match (attrs.select("__toString"), attrs.select("outPath")) {
                    (None, None) => Err(ErrorKind::NotCoercibleToString { from: "set", kind }),

                    (Some(f), _) => {
                        // use a closure here to deal with the thunk borrow we need to do below
                        let call_to_string = |value: &Value, vm: &mut VM| {
                            // Leave self on the stack as an argument to the function call.
                            vm.push(self.clone());
                            vm.call_value(value)?;
                            let result = vm.pop();

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
                            call_to_string(&guard, vm)
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

            (Value::Path(_), _)
            | (Value::Attrs(_), _)
            | (Value::Closure(_), _)
            | (Value::Builtin(_), _)
            | (Value::Null, _)
            | (Value::Bool(_), _)
            | (Value::Integer(_), _)
            | (Value::Float(_), _)
            | (Value::List(_), _) => Err(ErrorKind::NotCoercibleToString {
                from: self.type_of(),
                kind,
            }),

            (Value::AttrNotFound, _)
            | (Value::Blueprint(_), _)
            | (Value::DeferredUpvalue(_), _)
            | (Value::UnresolvedPath(_), _) => {
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
            | Value::AttrNotFound
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_)
            | Value::UnresolvedPath(_) => "internal",
        }
    }

    gen_cast!(as_bool, bool, "bool", Value::Bool(b), *b);
    gen_cast!(as_int, i64, "int", Value::Integer(x), *x);
    gen_cast!(as_float, f64, "float", Value::Float(x), *x);
    gen_cast!(to_str, NixString, "string", Value::String(s), s.clone());
    gen_cast!(to_attrs, Box<NixAttrs>, "set", Value::Attrs(a), a.clone());
    gen_cast!(to_list, NixList, "list", Value::List(l), l.clone());
    gen_cast!(
        as_closure,
        Rc<Closure>,
        "lambda",
        Value::Closure(c),
        c.clone()
    );

    gen_cast_mut!(as_list_mut, NixList, "list", List);

    gen_is!(is_path, Value::Path(_));
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
            (Value::String(s1), Value::String(s2)) => Ok(s1 == s2),
            (Value::Path(p1), Value::Path(p2)) => Ok(p1 == p2),

            // Numerical comparisons (they work between float & int)
            (Value::Integer(i1), Value::Integer(i2)) => Ok(i1 == i2),
            (Value::Integer(i), Value::Float(f)) => Ok(*i as f64 == *f),
            (Value::Float(f1), Value::Float(f2)) => Ok(f1 == f2),
            (Value::Float(f), Value::Integer(i)) => Ok(*i as f64 == *f),

            (Value::Attrs(_), Value::Attrs(_))
            | (Value::List(_), Value::List(_))
            | (Value::Thunk(_), _)
            | (_, Value::Thunk(_)) => Ok(vm.nix_eq(self.clone(), other.clone(), false)?),

            // Everything else is either incomparable (e.g. internal
            // types) or false.
            _ => Ok(false),
        }
    }

    /// Compare `self` against other using (fallible) Nix ordering semantics.
    pub fn nix_cmp(&self, other: &Self, vm: &mut VM) -> Result<Option<Ordering>, ErrorKind> {
        match (self, other) {
            // same types
            (Value::Integer(i1), Value::Integer(i2)) => Ok(i1.partial_cmp(i2)),
            (Value::Float(f1), Value::Float(f2)) => Ok(f1.partial_cmp(f2)),
            (Value::String(s1), Value::String(s2)) => Ok(s1.partial_cmp(s2)),
            (Value::List(l1), Value::List(l2)) => {
                for i in 0.. {
                    if i == l2.len() {
                        return Ok(Some(Ordering::Greater));
                    } else if i == l1.len() {
                        return Ok(Some(Ordering::Less));
                    } else if !vm.nix_eq(l1[i].clone(), l2[i].clone(), true)? {
                        return l1[i].force(vm)?.nix_cmp(&*l2[i].force(vm)?, vm);
                    }
                }

                unreachable!()
            }

            // different types
            (Value::Integer(i1), Value::Float(f2)) => Ok((*i1 as f64).partial_cmp(f2)),
            (Value::Float(f1), Value::Integer(i2)) => Ok(f1.partial_cmp(&(*i2 as f64))),

            // unsupported types
            (lhs, rhs) => Err(ErrorKind::Incomparable {
                lhs: lhs.type_of(),
                rhs: rhs.type_of(),
            }),
        }
    }

    /// Ensure `self` is forced if it is a thunk, and return a reference to the resulting value.
    pub fn force(&self, vm: &mut VM) -> Result<ForceResult, ErrorKind> {
        match self {
            Self::Thunk(thunk) => {
                thunk.force(vm)?;
                Ok(ForceResult::ForcedThunk(thunk.value()))
            }
            _ => Ok(ForceResult::Immediate(self)),
        }
    }

    /// Ensure `self` is *deeply* forced, including all recursive sub-values
    pub(crate) fn deep_force(
        &self,
        vm: &mut VM,
        thunk_set: &mut ThunkSet,
    ) -> Result<(), ErrorKind> {
        match self {
            Value::Null
            | Value::Bool(_)
            | Value::Integer(_)
            | Value::Float(_)
            | Value::String(_)
            | Value::Path(_)
            | Value::Closure(_)
            | Value::Builtin(_)
            | Value::AttrNotFound
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_)
            | Value::UnresolvedPath(_) => Ok(()),
            Value::Attrs(a) => {
                for (_, v) in a.iter() {
                    v.deep_force(vm, thunk_set)?;
                }
                Ok(())
            }
            Value::List(l) => {
                for val in l {
                    val.deep_force(vm, thunk_set)?;
                }
                Ok(())
            }
            Value::Thunk(thunk) => {
                if !thunk_set.insert(thunk) {
                    return Ok(());
                }

                thunk.force(vm)?;
                let value = thunk.value().clone();
                value.deep_force(vm, thunk_set)
            }
        }
    }

    /// Explain a value in a human-readable way, e.g. by presenting
    /// the docstrings of functions if present.
    pub fn explain(&self) -> String {
        match self {
            Value::Null => "the 'null' value".into(),
            Value::Bool(b) => format!("the boolean value '{}'", b),
            Value::Integer(i) => format!("the integer '{}'", i),
            Value::Float(f) => format!("the float '{}'", f),
            Value::String(s) => format!("the string '{}'", s),
            Value::Path(p) => format!("the path '{}'", p.to_string_lossy()),
            Value::Attrs(attrs) => format!("a {}-item attribute set", attrs.len()),
            Value::List(list) => format!("a {}-item list", list.len()),

            Value::Closure(f) => {
                if let Some(name) = &f.lambda.name {
                    format!("the user-defined Nix function '{}'", name)
                } else {
                    "a user-defined Nix function".to_string()
                }
            }

            Value::Builtin(b) => {
                let mut out = format!("the builtin function '{}'", b.name());
                if let Some(docs) = b.documentation() {
                    out.push_str("\n\n");
                    out.push_str(docs);
                }
                out
            }

            // TODO: handle suspended thunks with a different explanation instead of panicking
            Value::Thunk(t) => t.value().explain(),

            Value::AttrNotFound
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_)
            | Value::UnresolvedPath(_) => "an internal Tvix evaluator value".into(),
        }
    }
}

trait TotalDisplay {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result;
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.total_fmt(f, &mut Default::default())
    }
}

impl TotalDisplay for Value {
    fn total_fmt(&self, f: &mut std::fmt::Formatter<'_>, set: &mut ThunkSet) -> std::fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(true) => f.write_str("true"),
            Value::Bool(false) => f.write_str("false"),
            Value::Integer(num) => write!(f, "{}", num),
            Value::String(s) => s.fmt(f),
            Value::Path(p) => p.display().fmt(f),
            Value::Attrs(attrs) => attrs.total_fmt(f, set),
            Value::List(list) => list.total_fmt(f, set),
            Value::Closure(_) => f.write_str("lambda"), // TODO: print position
            Value::Builtin(builtin) => builtin.fmt(f),

            // Nix prints floats with a maximum precision of 5 digits
            // only.
            Value::Float(num) => {
                write!(f, "{}", format!("{:.5}", num).trim_end_matches(['.', '0']))
            }

            // internal types
            Value::AttrNotFound => f.write_str("internal[not found]"),
            Value::Blueprint(_) => f.write_str("internal[blueprint]"),
            Value::DeferredUpvalue(_) => f.write_str("internal[deferred_upvalue]"),
            Value::UnresolvedPath(_) => f.write_str("internal[unresolved_path]"),

            // Delegate thunk display to the type, as it must handle
            // the case of already evaluated or cyclic thunks.
            Value::Thunk(t) => t.total_fmt(f, set),
        }
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Value::Bool(b)
    }
}

impl From<i64> for Value {
    fn from(i: i64) -> Self {
        Self::Integer(i)
    }
}

impl From<f64> for Value {
    fn from(i: f64) -> Self {
        Self::Float(i)
    }
}

impl From<PathBuf> for Value {
    fn from(path: PathBuf) -> Self {
        Self::Path(path)
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
    use imbl::vector;

    mod nix_eq {
        use crate::observer::NoOpObserver;

        use super::*;
        use proptest::prelude::ProptestConfig;
        use test_strategy::proptest;

        #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
        fn reflexive(x: Value) {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(
                Default::default(),
                Box::new(crate::DummyIO),
                &mut observer,
                Default::default(),
            );

            assert!(x.nix_eq(&x, &mut vm).unwrap())
        }

        #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
        fn symmetric(x: Value, y: Value) {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(
                Default::default(),
                Box::new(crate::DummyIO),
                &mut observer,
                Default::default(),
            );

            assert_eq!(
                x.nix_eq(&y, &mut vm).unwrap(),
                y.nix_eq(&x, &mut vm).unwrap()
            )
        }

        #[proptest(ProptestConfig { cases: 5, ..Default::default() })]
        fn transitive(x: Value, y: Value, z: Value) {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(
                Default::default(),
                Box::new(crate::DummyIO),
                &mut observer,
                Default::default(),
            );

            if x.nix_eq(&y, &mut vm).unwrap() && y.nix_eq(&z, &mut vm).unwrap() {
                assert!(x.nix_eq(&z, &mut vm).unwrap())
            }
        }

        #[test]
        fn list_int_float_fungibility() {
            let mut observer = NoOpObserver {};
            let mut vm = VM::new(
                Default::default(),
                Box::new(crate::DummyIO),
                &mut observer,
                Default::default(),
            );

            let v1 = Value::List(NixList::from(vector![Value::Integer(1)]));
            let v2 = Value::List(NixList::from(vector![Value::Float(1.0)]));

            assert!(v1.nix_eq(&v2, &mut vm).unwrap())
        }
    }
}
