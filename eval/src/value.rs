//! This module implements the backing representation of runtime
//! values in the Nix language.

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
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NumberPair {
    Floats(f64, f64),
    Integer(i64, i64),
}
