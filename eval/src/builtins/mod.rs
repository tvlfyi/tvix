//! This module implements the builtins exposed in the Nix language.
//!
//! See //tvix/eval/docs/builtins.md for a some context on the
//! available builtins in Nix.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    rc::Rc,
};

use crate::{
    errors::ErrorKind,
    value::{Builtin, CoercionKind, NixAttrs, NixList, NixString, Value},
    vm::VM,
};

use crate::arithmetic_op;

/// Helper macro to ensure that a value has been forced. The structure
/// of this is a little cumbersome as there are different reference
/// types depending on whether the value is inside a thunk or not.
macro_rules! force {
    ( $vm:ident, $src:expr, $value:ident, $body:block ) => {
        if let Value::Thunk(thunk) = $src {
            thunk.force($vm)?;
            let guard = thunk.value();
            let $value: &Value = &guard;
            $body
        } else {
            let $value: &Value = $src;
            $body
        }
    };

    ( $vm:ident, $value:ident, $body:block ) => {
        force!($vm, &$value, $value, $body)
    };
}

/// Coerce a Nix Value to a plain path, e.g. in order to access the file it
/// points to in an I/O builtin. This coercion can _never_ be performed in
/// a Nix program directly (i.e. the trick `path: /. + path` to convert from
/// a string to a path wouldn't hit this code), so the target file
/// doesn't need to be realised or imported into the Nix store.
pub fn coerce_value_to_path(v: &Value, vm: &mut VM) -> Result<PathBuf, ErrorKind> {
    force!(vm, v, value, {
        match value {
            Value::Thunk(t) => coerce_value_to_path(&t.value(), vm),
            Value::Path(p) => Ok(p.clone()),
            _ => value
                .coerce_to_string(CoercionKind::Weak, vm)
                .map(|s| PathBuf::from(s.as_str()))
                .and_then(|path| {
                    if path.is_absolute() {
                        Ok(path)
                    } else {
                        Err(ErrorKind::NotAnAbsolutePath(path))
                    }
                }),
        }
    })
}

/// Return all pure builtins, that is all builtins that do not rely on
/// I/O outside of the VM and which can be used in any contexts (e.g.
/// WASM).
fn pure_builtins() -> Vec<Builtin> {
    vec![
        Builtin::new("add", 2, |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, +)
        }),
        Builtin::new("abort", 1, |mut args, _| {
            return Err(ErrorKind::Abort(
                args.pop().unwrap().to_str()?.as_str().to_owned(),
            ));
        }),
        Builtin::new("attrNames", 1, |args, vm| {
            force!(vm, &args[0], value, {
                let xs = value.to_attrs()?;
                let mut output = Vec::with_capacity(xs.len());

                for (key, _val) in xs.iter() {
                    output.push(Value::String(key.clone()));
                }

                Ok(Value::List(NixList::construct(output.len(), output)))
            })
        }),
        Builtin::new("attrValues", 1, |args, vm| {
            force!(vm, &args[0], value, {
                let xs = value.to_attrs()?;
                let mut output = Vec::with_capacity(xs.len());

                for (_key, val) in xs.iter() {
                    output.push(val.clone());
                }

                Ok(Value::List(NixList::construct(output.len(), output)))
            })
        }),
        Builtin::new("bitAnd", 2, |args, vm| {
            force!(vm, &args[0], x, {
                force!(vm, &args[1], y, {
                    Ok(Value::Integer(x.as_int()? & y.as_int()?))
                })
            })
        }),
        Builtin::new("bitOr", 2, |args, vm| {
            force!(vm, &args[0], x, {
                force!(vm, &args[1], y, {
                    Ok(Value::Integer(x.as_int()? | y.as_int()?))
                })
            })
        }),
        Builtin::new("bitXor", 2, |args, vm| {
            force!(vm, &args[0], x, {
                force!(vm, &args[1], y, {
                    Ok(Value::Integer(x.as_int()? ^ y.as_int()?))
                })
            })
        }),
        Builtin::new("catAttrs", 2, |mut args, _| {
            let list = args.pop().unwrap().to_list()?;
            let key = args.pop().unwrap().to_str()?;
            let mut output = vec![];

            for set in list.into_iter() {
                if let Some(value) = set.to_attrs()?.select(key.as_str()) {
                    output.push(value.clone());
                }
            }

            Ok(Value::List(NixList::construct(output.len(), output)))
        }),
        Builtin::new("div", 2, |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, /)
        }),
        Builtin::new("elemAt", 2, |args, vm| {
            force!(vm, &args[0], value, {
                let xs = value.to_list()?;
                let i = args[1].as_int()?;
                if i < 0 {
                    Err(ErrorKind::IndexOutOfBounds { index: i })
                } else {
                    match xs.get(i as usize) {
                        Some(x) => Ok(x.clone()),
                        None => Err(ErrorKind::IndexOutOfBounds { index: i }),
                    }
                }
            })
        }),
        Builtin::new("length", 1, |args, vm| {
            if let Value::Thunk(t) = &args[0] {
                t.force(vm)?;
            }
            Ok(Value::Integer(args[0].to_list()?.len() as i64))
        }),
        Builtin::new("head", 1, |args, vm| {
            force!(vm, &args[0], xs, {
                match xs.to_list()?.get(0) {
                    Some(x) => Ok(x.clone()),
                    None => Err(ErrorKind::IndexOutOfBounds { index: 0 }),
                }
            })
        }),
        Builtin::new("isAttrs", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::Attrs(_))))
        }),
        Builtin::new("isBool", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::Bool(_))))
        }),
        Builtin::new("isFloat", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::Float(_))))
        }),
        Builtin::new("isFunction", 1, |args, _| {
            Ok(Value::Bool(matches!(
                args[0],
                Value::Closure(_) | Value::Builtin(_)
            )))
        }),
        Builtin::new("isInt", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::Integer(_))))
        }),
        Builtin::new("isList", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::List(_))))
        }),
        Builtin::new("isNull", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::Null)))
        }),
        Builtin::new("isPath", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::Path(_))))
        }),
        Builtin::new("isString", 1, |args, _| {
            Ok(Value::Bool(matches!(args[0], Value::String(_))))
        }),
        Builtin::new("mul", 2, |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, *)
        }),
        Builtin::new("sub", 2, |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, -)
        }),
        Builtin::new("throw", 1, |mut args, _| {
            return Err(ErrorKind::Throw(
                args.pop().unwrap().to_str()?.as_str().to_owned(),
            ));
        }),
        Builtin::new("toString", 1, |args, vm| {
            args[0]
                .coerce_to_string(CoercionKind::Strong, vm)
                .map(Value::String)
        }),
        Builtin::new("typeOf", 1, |args, vm| {
            force!(vm, &args[0], value, {
                Ok(Value::String(value.type_of().into()))
            })
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
