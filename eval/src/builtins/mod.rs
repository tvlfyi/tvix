//! This module implements the builtins exposed in the Nix language.
//!
//! See //tvix/eval/docs/builtins.md for a some context on the
//! available builtins in Nix.

use std::{
    cmp,
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    rc::Rc,
};

use crate::{
    errors::ErrorKind,
    upvalues::UpvalueCarrier,
    value::{Builtin, Closure, CoercionKind, NixAttrs, NixList, NixString, Value},
    vm::VM,
};

use crate::arithmetic_op;

use self::versions::VersionPartsIter;

pub mod versions;

/// Coerce a Nix Value to a plain path, e.g. in order to access the file it
/// points to in an I/O builtin. This coercion can _never_ be performed in
/// a Nix program directly (i.e. the trick `path: /. + path` to convert from
/// a string to a path wouldn't hit this code), so the target file
/// doesn't need to be realised or imported into the Nix store.
pub fn coerce_value_to_path(v: &Value, vm: &mut VM) -> Result<PathBuf, ErrorKind> {
    let value = v.force(vm)?;
    match &*value {
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
}

/// Return all pure builtins, that is all builtins that do not rely on
/// I/O outside of the VM and which can be used in any contexts (e.g.
/// WASM).
fn pure_builtins() -> Vec<Builtin> {
    vec![
        Builtin::new("add", &[true, true], |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, +)
        }),
        Builtin::new("abort", &[true], |args, _| {
            return Err(ErrorKind::Abort(args[0].to_str()?.to_string()));
        }),
        Builtin::new("attrNames", &[true], |args, _| {
            let xs = args[0].to_attrs()?;
            let mut output = Vec::with_capacity(xs.len());

            for (key, _val) in xs.iter() {
                output.push(Value::String(key.clone()));
            }

            Ok(Value::List(NixList::construct(output.len(), output)))
        }),
        Builtin::new("attrValues", &[true], |args, _| {
            let xs = args[0].to_attrs()?;
            let mut output = Vec::with_capacity(xs.len());

            for (_key, val) in xs.iter() {
                output.push(val.clone());
            }

            Ok(Value::List(NixList::construct(output.len(), output)))
        }),
        Builtin::new("bitAnd", &[true, true], |args, _| {
            Ok(Value::Integer(args[0].as_int()? & args[1].as_int()?))
        }),
        Builtin::new("bitOr", &[true, true], |args, _| {
            Ok(Value::Integer(args[0].as_int()? | args[1].as_int()?))
        }),
        Builtin::new("bitXor", &[true, true], |args, _| {
            Ok(Value::Integer(args[0].as_int()? ^ args[1].as_int()?))
        }),
        Builtin::new("catAttrs", &[true, true], |args, vm| {
            let key = args[0].to_str()?;
            let list = args[1].to_list()?;
            let mut output = vec![];

            for item in list.into_iter() {
                let set = item.force(vm)?.to_attrs()?;
                if let Some(value) = set.select(key.as_str()) {
                    output.push(value.clone());
                }
            }

            Ok(Value::List(NixList::construct(output.len(), output)))
        }),
        Builtin::new("compareVersions", &[true, true], |args, _| {
            let s1 = args[0].to_str()?;
            let s1 = VersionPartsIter::new(s1.as_str());
            let s2 = args[1].to_str()?;
            let s2 = VersionPartsIter::new(s2.as_str());

            match s1.cmp(s2) {
                std::cmp::Ordering::Less => Ok(Value::Integer(-1)),
                std::cmp::Ordering::Equal => Ok(Value::Integer(0)),
                std::cmp::Ordering::Greater => Ok(Value::Integer(1)),
            }
        }),
        Builtin::new("div", &[true, true], |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, /)
        }),
        Builtin::new("elemAt", &[true, true], |args, _| {
            let xs = args[0].to_list()?;
            let i = args[1].as_int()?;
            if i < 0 {
                Err(ErrorKind::IndexOutOfBounds { index: i })
            } else {
                match xs.get(i as usize) {
                    Some(x) => Ok(x.clone()),
                    None => Err(ErrorKind::IndexOutOfBounds { index: i }),
                }
            }
        }),
        Builtin::new("getAttr", &[true, true], |args, _| {
            let k = args[0].to_str()?;
            let xs = args[1].to_attrs()?;

            match xs.select(k.as_str()) {
                Some(x) => Ok(x.clone()),
                None => Err(ErrorKind::AttributeNotFound {
                    name: k.to_string(),
                }),
            }
        }),
        Builtin::new("length", &[true], |args, _| {
            Ok(Value::Integer(args[0].to_list()?.len() as i64))
        }),
        Builtin::new("map", &[true, true], |args, vm| {
            let list: NixList = args[1].to_list()?;
            let func: Closure = args[0].to_closure()?;

            list.into_iter()
                .map(|val| {
                    // Leave the argument on the stack before calling the
                    // function.
                    vm.push(val);
                    vm.call(func.lambda(), func.upvalues().clone(), 1)
                })
                .collect::<Result<Vec<Value>, _>>()
                .map(|list| Value::List(NixList::from(list)))
                .map_err(Into::into)
        }),
        Builtin::new("hasAttr", &[true, true], |args, _| {
            let k = args[0].to_str()?;
            let xs = args[1].to_attrs()?;

            Ok(Value::Bool(xs.contains(k.as_str())))
        }),
        Builtin::new("head", &[true], |args, _| match args[0].to_list()?.get(0) {
            Some(x) => Ok(x.clone()),
            None => Err(ErrorKind::IndexOutOfBounds { index: 0 }),
        }),
        // For `is*` predicates we force manually, as Value::force also unwraps any Thunks
        Builtin::new("isAttrs", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Attrs(_))))
        }),
        Builtin::new("isBool", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Bool(_))))
        }),
        Builtin::new("isFloat", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Float(_))))
        }),
        Builtin::new("isFunction", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(
                *value,
                Value::Closure(_) | Value::Builtin(_)
            )))
        }),
        Builtin::new("isInt", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Integer(_))))
        }),
        Builtin::new("isList", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::List(_))))
        }),
        Builtin::new("isNull", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Null)))
        }),
        Builtin::new("isPath", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Path(_))))
        }),
        Builtin::new("isString", &[false], |args, vm| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::String(_))))
        }),
        Builtin::new("mul", &[true, true], |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, *)
        }),
        Builtin::new("sub", &[true, true], |mut args, _| {
            let b = args.pop().unwrap();
            let a = args.pop().unwrap();
            arithmetic_op!(a, b, -)
        }),
        Builtin::new("substring", &[true, true, true], |args, _| {
            let beg = args[0].as_int()?;
            let len = args[1].as_int()?;
            let x = args[2].to_str()?;

            if beg < 0 {
                return Err(ErrorKind::IndexOutOfBounds { index: beg });
            }
            let beg = beg as usize;

            // Nix doesn't assert that the length argument is
            // non-negative when the starting index is GTE the
            // string's length.
            if beg >= x.as_str().len() {
                return Ok(Value::String("".into()));
            }

            if len < 0 {
                return Err(ErrorKind::NegativeLength { length: len });
            }

            let len = len as usize;
            let end = cmp::min(beg + len, x.as_str().len());

            Ok(Value::String(
                x.as_str()[(beg as usize)..(end as usize)].into(),
            ))
        }),
        Builtin::new("tail", &[true], |args, _| {
            let xs = args[0].to_list()?;

            if xs.len() == 0 {
                Err(ErrorKind::TailEmptyList)
            } else {
                let output = xs.into_iter().skip(1).collect::<Vec<_>>();
                Ok(Value::List(NixList::construct(output.len(), output)))
            }
        }),
        Builtin::new("throw", &[true], |args, _| {
            return Err(ErrorKind::Throw(args[0].to_str()?.to_string()));
        }),
        // coerce_to_string forces for us
        Builtin::new("toString", &[false], |args, vm| {
            args[0]
                .coerce_to_string(CoercionKind::Strong, vm)
                .map(Value::String)
        }),
        Builtin::new("typeOf", &[false], |args, vm| {
            // We force manually here because it also unwraps the Thunk
            // representation, if any.
            // TODO(sterni): it'd be nice if we didn't have to worry about this
            let value = args[0].force(vm)?;
            Ok(Value::String(value.type_of().into()))
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
