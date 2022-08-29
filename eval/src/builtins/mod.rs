//! This module implements the builtins exposed in the Nix language.
//!
//! See //tvix/eval/docs/builtins.md for a some context on the
//! available builtins in Nix.

use std::{
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

use crate::{
    errors::ErrorKind,
    value::{Builtin, NixAttrs, NixList, NixString, Value},
};

use crate::arithmetic_op;

fn pure_builtins() -> Vec<Builtin> {
    vec![
        Builtin::new("add", 2, |mut args| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            Ok(arithmetic_op!(a, b, +))
        }),
        Builtin::new("abort", 1, |mut args| {
            return Err(
                ErrorKind::Abort(args.pop().unwrap().to_string()?.as_str().to_owned()).into(),
            );
        }),
        Builtin::new("catAttrs", 2, |mut args| {
            let list = args.pop().unwrap().to_list()?;
            let key = args.pop().unwrap().to_string()?;
            let mut output = vec![];

            for set in list.into_iter() {
                if let Some(value) = set.to_attrs()?.select(key.as_str()) {
                    output.push(value.clone());
                }
            }

            Ok(Value::List(NixList::construct(output.len(), output)))
        }),
        Builtin::new("isAttrs", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::Attrs(_))))
        }),
        Builtin::new("isBool", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::Bool(_))))
        }),
        Builtin::new("isFloat", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::Float(_))))
        }),
        Builtin::new("isFunction", 1, |args| {
            Ok(Value::Bool(matches!(
                args[0],
                Value::Closure(_) | Value::Builtin(_)
            )))
        }),
        Builtin::new("isInt", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::Integer(_))))
        }),
        Builtin::new("isList", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::List(_))))
        }),
        Builtin::new("isNull", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::Null)))
        }),
        Builtin::new("isPath", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::Path(_))))
        }),
        Builtin::new("isString", 1, |args| {
            Ok(Value::Bool(matches!(args[0], Value::String(_))))
        }),
        Builtin::new("sub", 2, |mut args| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            Ok(arithmetic_op!(a, b, -))
        }),
        Builtin::new("throw", 1, |mut args| {
            return Err(
                ErrorKind::Throw(args.pop().unwrap().to_string()?.as_str().to_owned()).into(),
            );
        }),
        Builtin::new("toString", 1, |args| {
            // TODO: toString is actually not the same as Display
            Ok(Value::String(format!("{}", args[0]).into()))
        }),
        Builtin::new("typeOf", 1, |args| {
            Ok(Value::String(args[0].type_of().into()))
        }),
    ]
}

fn builtins_set() -> NixAttrs {
    let mut map: BTreeMap<NixString, Value> = BTreeMap::new();

    for builtin in pure_builtins() {
        map.insert(builtin.name().into(), Value::Builtin(builtin));
    }

    NixAttrs::from_map(map)
}

/// Set of Nix builtins that are globally available.
pub fn global_builtins() -> HashMap<&'static str, Value> {
    let builtins = builtins_set();
    let mut globals: HashMap<&'static str, Value> = HashMap::new();

    // known global builtins from the builtins set.
    for global in &[
        "abort",
        "baseNameOf",
        "derivation",
        "derivationStrict",
        "dirOf",
        "fetchGit",
        "fetchMercurial",
        "fetchTarball",
        "fromTOML",
        "import",
        "isNull",
        "map",
        "placeholder",
        "removeAttrs",
        "scopedImport",
        "throw",
        "toString",
    ] {
        if let Some(builtin) = builtins.select(global) {
            globals.insert(global, builtin.clone());
        }
    }

    globals.insert("builtins", Value::Attrs(Rc::new(builtins)));

    globals
}
