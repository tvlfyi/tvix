//! This module implements the backing representation of runtime
//! values in the Nix language.
use std::fmt::Display;

use crate::errors::{Error, EvalResult};

mod attrs;
mod string;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
}

impl Value {
    pub fn is_number(&self) -> bool {
        match self {
            Value::Integer(_) => true,
            Value::Float(_) => true,
            _ => false,
        }
    }

    pub fn type_of(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Integer(_) => "int",
            Value::Float(_) => "float",
        }
    }

    pub fn as_bool(self) -> EvalResult<bool> {
        match self {
            Value::Bool(b) => Ok(b),
            other => Err(Error::TypeError {
                expected: "bool",
                actual: other.type_of(),
            }),
        }
    }
}

impl Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Null => f.write_str("null"),
            Value::Bool(true) => f.write_str("true"),
            Value::Bool(false) => f.write_str("false"),
            Value::Integer(num) => f.write_fmt(format_args!("{}", num)),
            Value::Float(num) => f.write_fmt(format_args!("{}", num)),
        }
    }
}
