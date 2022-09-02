//! This module implements the backing representation of runtime
//! values in the Nix language.
use std::rc::Rc;
use std::{fmt::Display, path::PathBuf};

mod attrs;
mod builtin;
mod function;
mod list;
mod string;
mod thunk;

use crate::errors::ErrorKind;
use crate::opcode::StackIdx;
pub use attrs::NixAttrs;
pub use builtin::Builtin;
pub use function::{Closure, Lambda};
pub use list::NixList;
pub use string::NixString;
pub use thunk::Thunk;

#[warn(variant_size_differences)]
#[derive(Clone, Debug)]
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

impl Value {
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
    gen_cast!(to_str, NixString, "string", Value::String(s), s.clone());
    gen_cast!(to_attrs, Rc<NixAttrs>, "set", Value::Attrs(a), a.clone());
    gen_cast!(to_list, NixList, "list", Value::List(l), l.clone());
    gen_cast!(to_closure, Closure, "lambda", Value::Closure(c), c.clone());

    gen_is!(is_number, Value::Integer(_) | Value::Float(_));
    gen_is!(is_bool, Value::Bool(_));
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

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            // Trivial comparisons
            (Value::Null, Value::Null) => true,
            (Value::Bool(b1), Value::Bool(b2)) => b1 == b2,
            (Value::List(l1), Value::List(l2)) => l1 == l2,
            (Value::String(s1), Value::String(s2)) => s1 == s2,

            // Numerical comparisons (they work between float & int)
            (Value::Integer(i1), Value::Integer(i2)) => i1 == i2,
            (Value::Integer(i), Value::Float(f)) => *i as f64 == *f,
            (Value::Float(f1), Value::Float(f2)) => f1 == f2,
            (Value::Float(f), Value::Integer(i)) => *i as f64 == *f,

            // Optimised attribute set comparison
            (Value::Attrs(a1), Value::Attrs(a2)) => Rc::ptr_eq(a1, a2) || { a1 == a2 },

            // If either value is a thunk, the inner value must be
            // compared instead. The compiler should ensure that
            // thunks under comparison have been forced, otherwise it
            // is a bug.
            (Value::Thunk(lhs), rhs) => &*lhs.value() == rhs,
            (lhs, Value::Thunk(rhs)) => lhs == &*rhs.value(),

            // Everything else is either incomparable (e.g. internal
            // types) or false.
            // TODO(tazjin): mirror Lambda equality behaviour
            _ => false,
        }
    }
}

fn type_error(expected: &'static str, actual: &Value) -> ErrorKind {
    ErrorKind::TypeError {
        expected,
        actual: actual.type_of(),
    }
}
