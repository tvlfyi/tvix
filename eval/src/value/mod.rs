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

use crate::errors::{ErrorKind, EvalResult};
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

impl Value {
    pub fn is_number(&self) -> bool {
        matches!(self, Value::Integer(_) | Value::Float(_))
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

    pub fn as_bool(&self) -> EvalResult<bool> {
        match self {
            Value::Bool(b) => Ok(*b),
            other => Err(ErrorKind::TypeError {
                expected: "bool",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn as_attrs(&self) -> EvalResult<&NixAttrs> {
        match self {
            Value::Attrs(attrs) => Ok(attrs),
            other => Err(ErrorKind::TypeError {
                expected: "set",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn as_str(&self) -> EvalResult<&str> {
        match self {
            Value::String(s) => Ok(s.as_str()),
            other => Err(ErrorKind::TypeError {
                expected: "string",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn as_list(&self) -> EvalResult<&NixList> {
        match self {
            Value::List(xs) => Ok(xs),
            other => Err(ErrorKind::TypeError {
                expected: "list",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn to_string(self) -> EvalResult<NixString> {
        match self {
            Value::String(s) => Ok(s),
            other => Err(ErrorKind::TypeError {
                expected: "string",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn to_attrs(self) -> EvalResult<Rc<NixAttrs>> {
        match self {
            Value::Attrs(s) => Ok(s),
            other => Err(ErrorKind::TypeError {
                expected: "set",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn to_list(self) -> EvalResult<NixList> {
        match self {
            Value::List(l) => Ok(l),
            other => Err(ErrorKind::TypeError {
                expected: "list",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn to_closure(self) -> EvalResult<Closure> {
        match self {
            Value::Closure(c) => Ok(c),
            other => Err(ErrorKind::TypeError {
                expected: "lambda",
                actual: other.type_of(),
            }
            .into()),
        }
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
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
