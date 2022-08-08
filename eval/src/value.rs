//! This module implements the backing representation of runtime
//! values in the Nix language.

use crate::errors::{Error, EvalResult};

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
