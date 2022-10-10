//! This module implements the builtins exposed in the Nix language.
//!
//! See //tvix/eval/docs/builtins.md for a some context on the
//! available builtins in Nix.

use std::cmp;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::{
    errors::ErrorKind,
    value::{Builtin, CoercionKind, NixAttrs, NixList, NixString, Value},
    vm::VM,
};

use crate::{arithmetic_op, cmp_op};

use self::versions::{VersionPart, VersionPartsIter};

#[cfg(feature = "impure")]
pub mod impure;
pub mod versions;

/// Coerce a Nix Value to a plain path, e.g. in order to access the file it
/// points to in an I/O builtin. This coercion can _never_ be performed in
/// a Nix program directly (i.e. the trick `path: /. + path` to convert from
/// a string to a path wouldn't hit this code), so the target file
/// doesn't need to be realised or imported into the Nix store.
#[allow(dead_code)] // TODO(sterni): remove this once the function is in use
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
        Builtin::new(
            "add",
            &[false, false],
            |args: Vec<Value>, vm: &mut VM| arithmetic_op!(&*args[0].force(vm)?, &*args[1].force(vm)?, +),
        ),
        Builtin::new("abort", &[true], |args: Vec<Value>, _: &mut VM| {
            Err(ErrorKind::Abort(args[0].to_str()?.to_string()))
        }),
        Builtin::new("all", &[true, true], |args: Vec<Value>, vm: &mut VM| {
            for value in args[1].to_list()?.into_iter() {
                let pred_result = vm.call_with(&args[0], [value])?;

                if !pred_result.force(vm)?.as_bool()? {
                    return Ok(Value::Bool(false));
                }
            }

            Ok(Value::Bool(true))
        }),
        Builtin::new("any", &[true, true], |args: Vec<Value>, vm: &mut VM| {
            for value in args[1].to_list()?.into_iter() {
                let pred_result = vm.call_with(&args[0], [value])?;

                if pred_result.force(vm)?.as_bool()? {
                    return Ok(Value::Bool(true));
                }
            }

            Ok(Value::Bool(false))
        }),
        Builtin::new("attrNames", &[true], |args: Vec<Value>, _: &mut VM| {
            let xs = args[0].to_attrs()?;
            let mut output = Vec::with_capacity(xs.len());

            for (key, _val) in xs.iter() {
                output.push(Value::String(key.clone()));
            }

            Ok(Value::List(NixList::construct(output.len(), output)))
        }),
        Builtin::new("attrValues", &[true], |args: Vec<Value>, _: &mut VM| {
            let xs = args[0].to_attrs()?;
            let mut output = Vec::with_capacity(xs.len());

            for (_key, val) in xs.iter() {
                output.push(val.clone());
            }

            Ok(Value::List(NixList::construct(output.len(), output)))
        }),
        Builtin::new("bitAnd", &[true, true], |args: Vec<Value>, _: &mut VM| {
            Ok(Value::Integer(args[0].as_int()? & args[1].as_int()?))
        }),
        Builtin::new("bitOr", &[true, true], |args: Vec<Value>, _: &mut VM| {
            Ok(Value::Integer(args[0].as_int()? | args[1].as_int()?))
        }),
        Builtin::new("bitXor", &[true, true], |args: Vec<Value>, _: &mut VM| {
            Ok(Value::Integer(args[0].as_int()? ^ args[1].as_int()?))
        }),
        Builtin::new(
            "catAttrs",
            &[true, true],
            |args: Vec<Value>, vm: &mut VM| {
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
            },
        ),
        Builtin::new(
            "compareVersions",
            &[true, true],
            |args: Vec<Value>, _: &mut VM| {
                let s1 = args[0].to_str()?;
                let s1 = VersionPartsIter::new_for_cmp(s1.as_str());
                let s2 = args[1].to_str()?;
                let s2 = VersionPartsIter::new_for_cmp(s2.as_str());

                match s1.cmp(s2) {
                    std::cmp::Ordering::Less => Ok(Value::Integer(-1)),
                    std::cmp::Ordering::Equal => Ok(Value::Integer(0)),
                    std::cmp::Ordering::Greater => Ok(Value::Integer(1)),
                }
            },
        ),
        Builtin::new("concatLists", &[true], |args: Vec<Value>, vm: &mut VM| {
            let list = args[0].to_list()?;
            let lists = list
                .into_iter()
                .map(|elem| {
                    let value = elem.force(vm)?;
                    value.to_list()
                })
                .collect::<Result<Vec<NixList>, ErrorKind>>()?;

            Ok(Value::List(NixList::from(
                lists.into_iter().flatten().collect::<Vec<Value>>(),
            )))
        }),
        Builtin::new(
            "concatMap",
            &[true, true],
            |args: Vec<Value>, vm: &mut VM| {
                let list = args[1].to_list()?;
                let mut res = Vec::new();
                for val in list {
                    res.extend(vm.call_with(&args[0], [val])?.force(vm)?.to_list()?);
                }
                Ok(Value::List(res.into()))
            },
        ),
        Builtin::new(
            "concatStringsSep",
            &[true, true],
            |args: Vec<Value>, vm: &mut VM| {
                let separator = args[0].to_str()?;
                let list = args[1].to_list()?;
                let mut res = String::new();
                for (i, val) in list.into_iter().enumerate() {
                    if i != 0 {
                        res.push_str(&separator);
                    }
                    res.push_str(&val.force(vm)?.coerce_to_string(CoercionKind::Weak, vm)?);
                }
                Ok(res.into())
            },
        ),
        Builtin::new(
            "div",
            &[false, false],
            |args: Vec<Value>, vm: &mut VM| arithmetic_op!(&*args[0].force(vm)?, &*args[1].force(vm)?, /),
        ),
        Builtin::new("elem", &[true, true], |args: Vec<Value>, vm: &mut VM| {
            for val in args[1].to_list()? {
                if val.nix_eq(&args[0], vm)? {
                    return Ok(true.into());
                }
            }
            Ok(false.into())
        }),
        Builtin::new("elemAt", &[true, true], |args: Vec<Value>, _: &mut VM| {
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
        Builtin::new("filter", &[true, true], |args: Vec<Value>, vm: &mut VM| {
            let list: NixList = args[1].to_list()?;

            list.into_iter()
                .filter_map(|elem| {
                    let result = match vm.call_with(&args[0], [elem.clone()]) {
                        Err(err) => return Some(Err(err)),
                        Ok(result) => result,
                    };

                    // Must be assigned to a local to avoid a borrowcheck
                    // failure related to the ForceResult destructor.
                    let result = match result.force(vm) {
                        Err(err) => Some(Err(vm.error(err))),
                        Ok(value) => match value.as_bool() {
                            Ok(true) => Some(Ok(elem)),
                            Ok(false) => None,
                            Err(err) => Some(Err(vm.error(err))),
                        },
                    };

                    result
                })
                .collect::<Result<Vec<Value>, _>>()
                .map(|list| Value::List(NixList::from(list)))
                .map_err(Into::into)
        }),
        Builtin::new(
            "foldl'",
            &[true, false, true],
            |mut args: Vec<Value>, vm: &mut VM| {
                let list = args.pop().unwrap().to_list()?;
                let mut res = args.pop().unwrap();
                let op = args.pop().unwrap();
                for val in list {
                    val.force(vm)?;
                    res = vm.call_with(&op, [val, res])?;
                }

                Ok(res)
            },
        ),
        Builtin::new("genList", &[true, true], |args: Vec<Value>, vm: &mut VM| {
            let len = args[1].as_int()?;
            (0..len)
                .map(|i| vm.call_with(&args[0], [i.into()]))
                .collect::<Result<Vec<Value>, _>>()
                .map(|list| Value::List(NixList::from(list)))
                .map_err(Into::into)
        }),
        Builtin::new("getAttr", &[true, true], |args: Vec<Value>, _: &mut VM| {
            let k = args[0].to_str()?;
            let xs = args[1].to_attrs()?;

            match xs.select(k.as_str()) {
                Some(x) => Ok(x.clone()),
                None => Err(ErrorKind::AttributeNotFound {
                    name: k.to_string(),
                }),
            }
        }),
        Builtin::new("length", &[true], |args: Vec<Value>, _: &mut VM| {
            Ok(Value::Integer(args[0].to_list()?.len() as i64))
        }),
        Builtin::new("map", &[true, true], |args: Vec<Value>, vm: &mut VM| {
            let list: NixList = args[1].to_list()?;

            list.into_iter()
                .map(|val| vm.call_with(&args[0], [val]))
                .collect::<Result<Vec<Value>, _>>()
                .map(|list| Value::List(NixList::from(list)))
                .map_err(Into::into)
        }),
        Builtin::new(
            "lessThan",
            &[false, false],
            |args: Vec<Value>, vm: &mut VM| cmp_op!(&*args[0].force(vm)?, &*args[1].force(vm)?, <),
        ),
        Builtin::new("hasAttr", &[true, true], |args: Vec<Value>, _: &mut VM| {
            let k = args[0].to_str()?;
            let xs = args[1].to_attrs()?;

            Ok(Value::Bool(xs.contains(k.as_str())))
        }),
        Builtin::new("head", &[true], |args: Vec<Value>, _: &mut VM| {
            match args[0].to_list()?.get(0) {
                Some(x) => Ok(x.clone()),
                None => Err(ErrorKind::IndexOutOfBounds { index: 0 }),
            }
        }),
        // For `is*` predicates we force manually, as Value::force also unwraps any Thunks
        Builtin::new("isAttrs", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Attrs(_))))
        }),
        Builtin::new("isBool", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Bool(_))))
        }),
        Builtin::new("isFloat", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Float(_))))
        }),
        Builtin::new("isFunction", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(
                *value,
                Value::Closure(_) | Value::Builtin(_)
            )))
        }),
        Builtin::new("isInt", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Integer(_))))
        }),
        Builtin::new("isList", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::List(_))))
        }),
        Builtin::new("isNull", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Null)))
        }),
        Builtin::new("isPath", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::Path(_))))
        }),
        Builtin::new("isString", &[false], |args: Vec<Value>, vm: &mut VM| {
            let value = args[0].force(vm)?;
            Ok(Value::Bool(matches!(*value, Value::String(_))))
        }),
        Builtin::new("listToAttrs", &[true], |args: Vec<Value>, vm: &mut VM| {
            let list = args[0].to_list()?;
            let mut map = BTreeMap::new();
            for val in list {
                let attrs = val.force(vm)?.to_attrs()?;
                let get = |key| {
                    attrs
                        .select(key)
                        .ok_or(ErrorKind::AttributeNotFound { name: key.into() })
                };
                let name = get("name")?.to_str()?;
                let value = get("value")?.clone();
                // Map entries earlier in the list take precedence over entries later in the list
                map.entry(name).or_insert(value);
            }
            Ok(Value::attrs(NixAttrs::from_map(map)))
        }),
        Builtin::new(
            "mul",
            &[false, false],
            |args: Vec<Value>, vm: &mut VM| arithmetic_op!(&*args[0].force(vm)?, &*args[1].force(vm)?, *),
        ),
        Builtin::new("splitVersion", &[true], |args: Vec<Value>, _: &mut VM| {
            let s = args[0].to_str()?;
            let s = VersionPartsIter::new(s.as_str());

            let parts = s
                .map(|s| {
                    Value::String(match s {
                        VersionPart::Number(n) => n.into(),
                        VersionPart::Word(w) => w.into(),
                    })
                })
                .collect::<Vec<Value>>();
            Ok(Value::List(NixList::construct(parts.len(), parts)))
        }),
        Builtin::new("stringLength", &[false], |args: Vec<Value>, vm: &mut VM| {
            // also forces the value
            let s = args[0].coerce_to_string(CoercionKind::Weak, vm)?;
            Ok(Value::Integer(s.as_str().len() as i64))
        }),
        Builtin::new(
            "sub",
            &[false, false],
            |args: Vec<Value>, vm: &mut VM| arithmetic_op!(&*args[0].force(vm)?, &*args[1].force(vm)?, -),
        ),
        Builtin::new(
            "substring",
            &[true, true, true],
            |args: Vec<Value>, _: &mut VM| {
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
            },
        ),
        Builtin::new("tail", &[true], |args: Vec<Value>, _: &mut VM| {
            let xs = args[0].to_list()?;

            if xs.len() == 0 {
                Err(ErrorKind::TailEmptyList)
            } else {
                let output = xs.into_iter().skip(1).collect::<Vec<_>>();
                Ok(Value::List(NixList::construct(output.len(), output)))
            }
        }),
        Builtin::new("throw", &[true], |args: Vec<Value>, _: &mut VM| {
            Err(ErrorKind::Throw(args[0].to_str()?.to_string()))
        }),
        Builtin::new(
            "trace",
            &[true, true],
            |mut args: Vec<Value>, _: &mut VM| {
                let value = args.pop().unwrap();
                let trace_value = args.pop().unwrap();
                // TODO(grfn): `trace` should be pluggable and capturable, probably via a method on
                // the VM
                println!("trace: {} :: {}", trace_value, trace_value.type_of());
                Ok(value)
            },
        ),
        Builtin::new("tryEval", &[false], |args: Vec<Value>, vm: &mut VM| {
            let mut res = BTreeMap::new();
            match args[0].force(vm) {
                Ok(value) => {
                    res.insert("value".into(), (*value).clone());
                    res.insert("success".into(), true.into());
                }
                Err(e) if e.is_catchable() => {
                    res.insert("value".into(), false.into());
                    res.insert("success".into(), false.into());
                }
                Err(e) => return Err(e),
            }
            Ok(Value::attrs(NixAttrs::from_map(res)))
        }),
        // coerce_to_string forces for us
        Builtin::new("toString", &[false], |args: Vec<Value>, vm: &mut VM| {
            args[0]
                .coerce_to_string(CoercionKind::Strong, vm)
                .map(Value::String)
        }),
        Builtin::new("typeOf", &[false], |args: Vec<Value>, vm: &mut VM| {
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

    // Pure-value builtins
    map.insert(
        "nixVersion".into(),
        Value::String("2.3-compat-tvix-0.1".into()),
    );

    let mut add_builtins = |builtins: Vec<Builtin>| {
        for builtin in builtins {
            map.insert(builtin.name().into(), Value::Builtin(builtin));
        }
    };

    add_builtins(pure_builtins());
    #[cfg(feature = "impure")]
    {
        map.extend(impure::builtins());
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

    globals.insert("builtins", Value::attrs(builtins));

    globals
}
