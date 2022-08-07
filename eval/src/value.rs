//! This module implements the backing representation of runtime
//! values in the Nix language.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
}
