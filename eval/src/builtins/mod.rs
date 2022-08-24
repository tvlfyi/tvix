//! This module implements the builtins exposed in the Nix language.
//!
//! See //tvix/eval/docs/builtins.md for a some context on the
//! available builtins in Nix.

use std::collections::HashMap;

use crate::value::{Builtin, Value};

macro_rules! builtin {
    ( $map:ident, $name:literal, $arity:literal, $body:expr ) => {
        $map.insert($name, Value::Builtin(Builtin::new($name, $arity, $body)));
    };
}

/// Set of Nix builtins that are globally available.
pub fn global_builtins() -> HashMap<&'static str, Value> {
    let mut globals = HashMap::new();

    builtin!(globals, "toString", 1, |args| {
        // TODO: toString is actually not the same as Display
        Ok(Value::String(format!("{}", args[0]).into()))
    });

    globals
}
