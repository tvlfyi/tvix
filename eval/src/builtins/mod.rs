//! This module implements the builtins exposed in the Nix language.
//!
//! See //tvix/eval/docs/builtins.md for a some context on the
//! available builtins in Nix.

use builtin_macros::builtins;
use genawaiter::rc::Gen;
use regex::Regex;
use std::cmp::{self, Ordering};
use std::collections::VecDeque;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use crate::arithmetic_op;
use crate::value::PointerEquality;
use crate::vm::generators::{self, GenCo};
use crate::warnings::WarningKind;
use crate::{
    errors::ErrorKind,
    value::{CoercionKind, NixAttrs, NixList, NixString, SharedThunkSet, Value},
};

use self::versions::{VersionPart, VersionPartsIter};

mod to_xml;
mod versions;

#[cfg(feature = "impure")]
mod impure;

#[cfg(feature = "impure")]
pub use impure::impure_builtins;

// we set TVIX_CURRENT_SYSTEM in build.rs
pub const CURRENT_PLATFORM: &str = env!("TVIX_CURRENT_SYSTEM");

/// Coerce a Nix Value to a plain path, e.g. in order to access the
/// file it points to via either `builtins.toPath` or an impure
/// builtin. This coercion can _never_ be performed in a Nix program
/// without using builtins (i.e. the trick `path: /. + path` to
/// convert from a string to a path wouldn't hit this code).
pub async fn coerce_value_to_path(co: &GenCo, v: Value) -> Result<PathBuf, ErrorKind> {
    let value = generators::request_force(co, v).await;
    if let Value::Path(p) = value {
        return Ok(*p);
    }

    let vs = generators::request_string_coerce(co, value, CoercionKind::Weak).await;
    let path = PathBuf::from(vs.as_str());
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(ErrorKind::NotAnAbsolutePath(path))
    }
}

#[builtins]
mod pure_builtins {
    use crate::value::PointerEquality;

    use super::*;

    #[builtin("abort")]
    async fn builtin_abort(co: GenCo, message: Value) -> Result<Value, ErrorKind> {
        Err(ErrorKind::Abort(message.to_str()?.to_string()))
    }

    #[builtin("add")]
    async fn builtin_add(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        arithmetic_op!(&x, &y, +)
    }

    #[builtin("all")]
    async fn builtin_all(co: GenCo, pred: Value, list: Value) -> Result<Value, ErrorKind> {
        for value in list.to_list()?.into_iter() {
            let pred_result = generators::request_call_with(&co, pred.clone(), [value]).await;
            let pred_result = generators::request_force(&co, pred_result).await;

            if !pred_result.as_bool()? {
                return Ok(Value::Bool(false));
            }
        }

        Ok(Value::Bool(true))
    }

    #[builtin("any")]
    async fn builtin_any(co: GenCo, pred: Value, list: Value) -> Result<Value, ErrorKind> {
        for value in list.to_list()?.into_iter() {
            let pred_result = generators::request_call_with(&co, pred.clone(), [value]).await;
            let pred_result = generators::request_force(&co, pred_result).await;

            if pred_result.as_bool()? {
                return Ok(Value::Bool(true));
            }
        }

        Ok(Value::Bool(false))
    }

    #[builtin("attrNames")]
    async fn builtin_attr_names(co: GenCo, set: Value) -> Result<Value, ErrorKind> {
        let xs = set.to_attrs()?;
        let mut output = Vec::with_capacity(xs.len());

        for (key, _val) in xs.iter() {
            output.push(Value::String(key.clone()));
        }

        Ok(Value::List(NixList::construct(output.len(), output)))
    }

    #[builtin("attrValues")]
    async fn builtin_attr_values(co: GenCo, set: Value) -> Result<Value, ErrorKind> {
        let xs = set.to_attrs()?;
        let mut output = Vec::with_capacity(xs.len());

        for (_key, val) in xs.iter() {
            output.push(val.clone());
        }

        Ok(Value::List(NixList::construct(output.len(), output)))
    }

    #[builtin("baseNameOf")]
    async fn builtin_base_name_of(co: GenCo, s: Value) -> Result<Value, ErrorKind> {
        let s = s.coerce_to_string(co, CoercionKind::Weak).await?.to_str()?;
        let result: String = s.rsplit_once('/').map(|(_, x)| x).unwrap_or(&s).into();
        Ok(result.into())
    }

    #[builtin("bitAnd")]
    async fn builtin_bit_and(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Integer(x.as_int()? & y.as_int()?))
    }

    #[builtin("bitOr")]
    async fn builtin_bit_or(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Integer(x.as_int()? | y.as_int()?))
    }

    #[builtin("bitXor")]
    async fn builtin_bit_xor(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Integer(x.as_int()? ^ y.as_int()?))
    }

    #[builtin("catAttrs")]
    async fn builtin_cat_attrs(co: GenCo, key: Value, list: Value) -> Result<Value, ErrorKind> {
        let key = key.to_str()?;
        let list = list.to_list()?;
        let mut output = vec![];

        for item in list.into_iter() {
            let set = generators::request_force(&co, item).await.to_attrs()?;

            if let Some(value) = set.select(key.as_str()) {
                output.push(value.clone());
            }
        }

        Ok(Value::List(NixList::construct(output.len(), output)))
    }

    #[builtin("ceil")]
    async fn builtin_ceil(co: GenCo, double: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Integer(double.as_float()?.ceil() as i64))
    }

    #[builtin("compareVersions")]
    async fn builtin_compare_versions(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        let s1 = x.to_str()?;
        let s1 = VersionPartsIter::new_for_cmp(s1.as_str());
        let s2 = y.to_str()?;
        let s2 = VersionPartsIter::new_for_cmp(s2.as_str());

        match s1.cmp(s2) {
            std::cmp::Ordering::Less => Ok(Value::Integer(-1)),
            std::cmp::Ordering::Equal => Ok(Value::Integer(0)),
            std::cmp::Ordering::Greater => Ok(Value::Integer(1)),
        }
    }

    #[builtin("concatLists")]
    async fn builtin_concat_lists(co: GenCo, lists: Value) -> Result<Value, ErrorKind> {
        let mut out = imbl::Vector::new();

        for value in lists.to_list()? {
            let list = generators::request_force(&co, value).await.to_list()?;
            out.extend(list.into_iter());
        }

        Ok(Value::List(out.into()))
    }

    #[builtin("concatMap")]
    async fn builtin_concat_map(co: GenCo, f: Value, list: Value) -> Result<Value, ErrorKind> {
        let list = list.to_list()?;
        let mut res = imbl::Vector::new();
        for val in list {
            let out = generators::request_call_with(&co, f.clone(), [val]).await;
            let out = generators::request_force(&co, out).await;
            res.extend(out.to_list()?);
        }
        Ok(Value::List(res.into()))
    }

    #[builtin("concatStringsSep")]
    async fn builtin_concat_strings_sep(
        co: GenCo,
        separator: Value,
        list: Value,
    ) -> Result<Value, ErrorKind> {
        let separator = separator.to_str()?;
        let list = list.to_list()?;
        let mut res = String::new();
        for (i, val) in list.into_iter().enumerate() {
            if i != 0 {
                res.push_str(&separator);
            }
            let s = generators::request_string_coerce(&co, val, CoercionKind::Weak).await;
            res.push_str(s.as_str());
        }
        Ok(res.into())
    }

    #[builtin("deepSeq")]
    async fn builtin_deep_seq(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        generators::request_deep_force(&co, x, SharedThunkSet::default()).await;
        Ok(y)
    }

    #[builtin("div")]
    async fn builtin_div(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        arithmetic_op!(&x, &y, /)
    }

    #[builtin("dirOf")]
    async fn builtin_dir_of(co: GenCo, s: Value) -> Result<Value, ErrorKind> {
        let is_path = s.is_path();
        let str = s.coerce_to_string(co, CoercionKind::Weak).await?.to_str()?;
        let result = str
            .rsplit_once('/')
            .map(|(x, _)| match x {
                "" => "/",
                _ => x,
            })
            .unwrap_or(".");
        if is_path {
            Ok(Value::Path(Box::new(result.into())))
        } else {
            Ok(result.into())
        }
    }

    #[builtin("elem")]
    async fn builtin_elem(co: GenCo, x: Value, xs: Value) -> Result<Value, ErrorKind> {
        for val in xs.to_list()? {
            if generators::check_equality(&co, x.clone(), val, PointerEquality::AllowAll).await? {
                return Ok(true.into());
            }
        }
        Ok(false.into())
    }

    #[builtin("elemAt")]
    async fn builtin_elem_at(co: GenCo, xs: Value, i: Value) -> Result<Value, ErrorKind> {
        let xs = xs.to_list()?;
        let i = i.as_int()?;
        if i < 0 {
            Err(ErrorKind::IndexOutOfBounds { index: i })
        } else {
            match xs.get(i as usize) {
                Some(x) => Ok(x.clone()),
                None => Err(ErrorKind::IndexOutOfBounds { index: i }),
            }
        }
    }

    #[builtin("filter")]
    async fn builtin_filter(co: GenCo, pred: Value, list: Value) -> Result<Value, ErrorKind> {
        let list: NixList = list.to_list()?;
        let mut out = imbl::Vector::new();

        for value in list {
            let result = generators::request_call_with(&co, pred.clone(), [value.clone()]).await;

            if generators::request_force(&co, result).await.as_bool()? {
                out.push_back(value);
            }
        }

        Ok(Value::List(out.into()))
        // list.into_iter()
        //     .filter_map(|elem| {
        //         let result = match vm.call_with(&pred, [elem.clone()]) {
        //             Err(err) => return Some(Err(err)),
        //             Ok(result) => result,
        //         };

        //         // Must be assigned to a local to avoid a borrowcheck
        //         // failure related to the ForceResult destructor.
        //         let result = match result.force(vm) {
        //             Err(err) => Some(Err(vm.error(err))),
        //             Ok(value) => match value.as_bool() {
        //                 Ok(true) => Some(Ok(elem)),
        //                 Ok(false) => None,
        //                 Err(err) => Some(Err(vm.error(err))),
        //             },
        //         };

        //         result
        //     })
        //     .collect::<Result<imbl::Vector<Value>, _>>()
        //     .map(|list| Value::List(NixList::from(list)))
        //     .map_err(Into::into)
    }

    #[builtin("floor")]
    async fn builtin_floor(co: GenCo, double: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Integer(double.as_float()?.floor() as i64))
    }

    #[builtin("foldl'")]
    async fn builtin_foldl(
        co: GenCo,
        op: Value,
        #[lazy] nul: Value,
        list: Value,
    ) -> Result<Value, ErrorKind> {
        let mut nul = nul;
        let list = list.to_list()?;
        for val in list {
            nul = generators::request_call_with(&co, op.clone(), [nul, val]).await;
            nul = generators::request_force(&co, nul).await;
        }

        Ok(nul)
    }

    #[builtin("functionArgs")]
    async fn builtin_function_args(co: GenCo, f: Value) -> Result<Value, ErrorKind> {
        let lambda = &f.as_closure()?.lambda();
        let formals = if let Some(formals) = &lambda.formals {
            formals
        } else {
            return Ok(Value::attrs(NixAttrs::empty()));
        };
        Ok(Value::attrs(NixAttrs::from_iter(
            formals.arguments.iter().map(|(k, v)| (k.clone(), (*v))),
        )))
    }

    #[builtin("fromJSON")]
    async fn builtin_from_json(co: GenCo, json: Value) -> Result<Value, ErrorKind> {
        let json_str = json.to_str()?;

        serde_json::from_str(&json_str).map_err(|err| err.into())
    }

    #[builtin("toJSON")]
    async fn builtin_to_json(co: GenCo, val: Value) -> Result<Value, ErrorKind> {
        // All thunks need to be evaluated before serialising, as the
        // data structure is fully traversed by the Serializer.
        let val = generators::request_deep_force(&co, val, SharedThunkSet::default()).await;
        let json_str = serde_json::to_string(&val)?;
        Ok(json_str.into())
    }

    #[builtin("fromTOML")]
    async fn builtin_from_toml(co: GenCo, toml: Value) -> Result<Value, ErrorKind> {
        let toml_str = toml.to_str()?;

        toml::from_str(&toml_str).map_err(|err| err.into())
    }

    #[builtin("genericClosure")]
    async fn builtin_generic_closure(co: GenCo, input: Value) -> Result<Value, ErrorKind> {
        let attrs = input.to_attrs()?;

        // The work set is maintained as a VecDeque because new items
        // are popped from the front.
        let mut work_set: VecDeque<Value> =
            generators::request_force(&co, attrs.select_required("startSet")?.clone())
                .await
                .to_list()?
                .into_iter()
                .collect();

        let operator = attrs.select_required("operator")?;

        let mut res = imbl::Vector::new();
        let mut done_keys: Vec<Value> = vec![];

        while let Some(val) = work_set.pop_front() {
            let val = generators::request_force(&co, val).await;
            let attrs = val.to_attrs()?;
            let key = attrs.select_required("key")?;

            if !bgc_insert_key(&co, key.clone(), &mut done_keys).await? {
                continue;
            }

            res.push_back(val.clone());

            let op_result = generators::request_force(
                &co,
                generators::request_call_with(&co, operator.clone(), [val]).await,
            )
            .await;

            work_set.extend(op_result.to_list()?.into_iter());
        }

        Ok(Value::List(NixList::from(res)))
    }

    #[builtin("genList")]
    async fn builtin_gen_list(
        co: GenCo,
        generator: Value,
        length: Value,
    ) -> Result<Value, ErrorKind> {
        let mut out = imbl::Vector::<Value>::new();
        let len = length.as_int()?;

        for i in 0..len {
            let val = generators::request_call_with(&co, generator.clone(), [i.into()]).await;
            out.push_back(val);
        }

        Ok(Value::List(out.into()))
    }

    #[builtin("getAttr")]
    async fn builtin_get_attr(co: GenCo, key: Value, set: Value) -> Result<Value, ErrorKind> {
        let k = key.to_str()?;
        let xs = set.to_attrs()?;

        match xs.select(k.as_str()) {
            Some(x) => Ok(x.clone()),
            None => Err(ErrorKind::AttributeNotFound {
                name: k.to_string(),
            }),
        }
    }

    #[builtin("groupBy")]
    async fn builtin_group_by(co: GenCo, f: Value, list: Value) -> Result<Value, ErrorKind> {
        let mut res: BTreeMap<NixString, imbl::Vector<Value>> = BTreeMap::new();
        for val in list.to_list()? {
            let key = generators::request_force(
                &co,
                generators::request_call_with(&co, f.clone(), [val.clone()]).await,
            )
            .await
            .to_str()?;

            res.entry(key)
                .or_insert_with(imbl::Vector::new)
                .push_back(val);
        }
        Ok(Value::attrs(NixAttrs::from_iter(
            res.into_iter()
                .map(|(k, v)| (k, Value::List(NixList::from(v)))),
        )))
    }

    #[builtin("hasAttr")]
    async fn builtin_has_attr(co: GenCo, key: Value, set: Value) -> Result<Value, ErrorKind> {
        let k = key.to_str()?;
        let xs = set.to_attrs()?;

        Ok(Value::Bool(xs.contains(k.as_str())))
    }

    #[builtin("head")]
    async fn builtin_head(co: GenCo, list: Value) -> Result<Value, ErrorKind> {
        match list.to_list()?.get(0) {
            Some(x) => Ok(x.clone()),
            None => Err(ErrorKind::IndexOutOfBounds { index: 0 }),
        }
    }

    #[builtin("intersectAttrs")]
    async fn builtin_intersect_attrs(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        let attrs1 = x.to_attrs()?;
        let attrs2 = y.to_attrs()?;
        let res = attrs2.iter().filter_map(|(k, v)| {
            if attrs1.contains(k) {
                Some((k.clone(), v.clone()))
            } else {
                None
            }
        });
        Ok(Value::attrs(NixAttrs::from_iter(res)))
    }

    #[builtin("isAttrs")]
    async fn builtin_is_attrs(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::Attrs(_))))
    }

    #[builtin("isBool")]
    async fn builtin_is_bool(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::Bool(_))))
    }

    #[builtin("isFloat")]
    async fn builtin_is_float(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::Float(_))))
    }

    #[builtin("isFunction")]
    async fn builtin_is_function(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(
            value,
            Value::Closure(_) | Value::Builtin(_)
        )))
    }

    #[builtin("isInt")]
    async fn builtin_is_int(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::Integer(_))))
    }

    #[builtin("isList")]
    async fn builtin_is_list(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::List(_))))
    }

    #[builtin("isNull")]
    async fn builtin_is_null(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::Null)))
    }

    #[builtin("isPath")]
    async fn builtin_is_path(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::Path(_))))
    }

    #[builtin("isString")]
    async fn builtin_is_string(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(value, Value::String(_))))
    }

    #[builtin("length")]
    async fn builtin_length(co: GenCo, list: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Integer(list.to_list()?.len() as i64))
    }

    #[builtin("lessThan")]
    async fn builtin_less_than(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        Ok(Value::Bool(matches!(
            x.nix_cmp_ordering(y, co).await?,
            Some(Ordering::Less)
        )))
    }

    #[builtin("listToAttrs")]
    async fn builtin_list_to_attrs(co: GenCo, list: Value) -> Result<Value, ErrorKind> {
        let list = list.to_list()?;
        let mut map = BTreeMap::new();
        for val in list {
            let attrs = generators::request_force(&co, val).await.to_attrs()?;
            let name = generators::request_force(&co, attrs.select_required("name")?.clone())
                .await
                .to_str()?;
            let value = attrs.select_required("value")?.clone();
            // Map entries earlier in the list take precedence over entries later in the list
            map.entry(name).or_insert(value);
        }
        Ok(Value::attrs(NixAttrs::from_iter(map.into_iter())))
    }

    #[builtin("map")]
    async fn builtin_map(co: GenCo, f: Value, list: Value) -> Result<Value, ErrorKind> {
        let mut out = imbl::Vector::<Value>::new();

        for val in list.to_list()? {
            let result = generators::request_call_with(&co, f.clone(), [val]).await;
            out.push_back(result)
        }

        Ok(Value::List(out.into()))
    }

    #[builtin("mapAttrs")]
    async fn builtin_map_attrs(co: GenCo, f: Value, attrs: Value) -> Result<Value, ErrorKind> {
        let attrs = attrs.to_attrs()?;
        let mut out = imbl::OrdMap::new();

        for (key, value) in attrs.into_iter() {
            let result =
                generators::request_call_with(&co, f.clone(), [key.clone().into(), value]).await;
            out.insert(key, result);
        }

        // let res =
        //     attrs
        //         .as_ref()
        //         .into_iter()
        //         .flat_map(|(key, value)| -> EvalResult<(NixString, Value)> {
        //             let value = vm.call_with(&f, [key.clone().into(), value.clone()])?;
        //             Ok((key.to_owned(), value))
        //         });
        Ok(Value::attrs(out.into()))
    }

    #[builtin("match")]
    async fn builtin_match(co: GenCo, regex: Value, str: Value) -> Result<Value, ErrorKind> {
        let s = str.to_str()?;
        let re = regex.to_str()?;
        let re: Regex = Regex::new(&format!("^{}$", re.as_str())).unwrap();
        match re.captures(&s) {
            Some(caps) => Ok(Value::List(
                caps.iter()
                    .skip(1)
                    .map(|grp| grp.map(|g| Value::from(g.as_str())).unwrap_or(Value::Null))
                    .collect::<imbl::Vector<Value>>()
                    .into(),
            )),
            None => Ok(Value::Null),
        }
    }

    #[builtin("mul")]
    async fn builtin_mul(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        arithmetic_op!(&x, &y, *)
    }

    #[builtin("parseDrvName")]
    async fn builtin_parse_drv_name(co: GenCo, s: Value) -> Result<Value, ErrorKind> {
        // This replicates cppnix's (mis?)handling of codepoints
        // above U+007f following 0x2d ('-')
        let s = s.to_str()?;
        let slice: &[u8] = s.as_str().as_ref();
        let (name, dash_and_version) = slice.split_at(
            slice
                .windows(2)
                .enumerate()
                .find_map(|x| match x {
                    (idx, [b'-', c1]) if !c1.is_ascii_alphabetic() => Some(idx),
                    _ => None,
                })
                .unwrap_or(slice.len()),
        );
        let version = dash_and_version
            .split_first()
            .map(|x| core::str::from_utf8(x.1))
            .unwrap_or(Ok(""))?;
        Ok(Value::attrs(NixAttrs::from_iter(
            [("name", core::str::from_utf8(name)?), ("version", version)].into_iter(),
        )))
    }

    #[builtin("partition")]
    async fn builtin_partition(co: GenCo, pred: Value, list: Value) -> Result<Value, ErrorKind> {
        let mut right: imbl::Vector<Value> = Default::default();
        let mut wrong: imbl::Vector<Value> = Default::default();

        let list: NixList = list.to_list()?;
        for elem in list {
            let result = generators::request_call_with(&co, pred.clone(), [elem.clone()]).await;

            if generators::request_force(&co, result).await.as_bool()? {
                right.push_back(elem);
            } else {
                wrong.push_back(elem);
            };
        }

        let res = [
            ("right", Value::List(NixList::from(right))),
            ("wrong", Value::List(NixList::from(wrong))),
        ];

        Ok(Value::attrs(NixAttrs::from_iter(res.into_iter())))
    }

    #[builtin("removeAttrs")]
    async fn builtin_remove_attrs(
        co: GenCo,
        attrs: Value,
        keys: Value,
    ) -> Result<Value, ErrorKind> {
        let attrs = attrs.to_attrs()?;
        let keys = keys
            .to_list()?
            .into_iter()
            .map(|v| v.to_str())
            .collect::<Result<HashSet<_>, _>>()?;
        let res = attrs.iter().filter_map(|(k, v)| {
            if !keys.contains(k) {
                Some((k.clone(), v.clone()))
            } else {
                None
            }
        });
        Ok(Value::attrs(NixAttrs::from_iter(res)))
    }

    #[builtin("replaceStrings")]
    async fn builtin_replace_strings(
        co: GenCo,
        from: Value,
        to: Value,
        s: Value,
    ) -> Result<Value, ErrorKind> {
        let from = from.to_list()?;
        for val in &from {
            generators::request_force(&co, val.clone()).await;
        }

        let to = to.to_list()?;
        for val in &to {
            generators::request_force(&co, val.clone()).await;
        }

        let string = s.to_str()?;

        let mut res = String::new();

        let mut i: usize = 0;
        let mut empty_string_replace = false;

        // This can't be implemented using Rust's string.replace() as
        // well as a map because we need to handle errors with results
        // as well as "reset" the iterator to zero for the replacement
        // everytime there's a successful match.
        // Also, Rust's string.replace allocates a new string
        // on every call which is not preferable.
        'outer: while i < string.len() {
            // Try a match in all the from strings
            for elem in std::iter::zip(from.iter(), to.iter()) {
                let from = elem.0.to_str()?;
                let to = elem.1.to_str()?;

                if i + from.len() >= string.len() {
                    continue;
                }

                // We already applied a from->to with an empty from
                // transformation.
                // Let's skip it so that we don't loop infinitely
                if empty_string_replace && from.as_str().is_empty() {
                    continue;
                }

                // if we match the `from` string, let's replace
                if &string[i..i + from.len()] == from.as_str() {
                    res += &to;
                    i += from.len();

                    // remember if we applied the empty from->to
                    empty_string_replace = from.as_str().is_empty();

                    continue 'outer;
                }
            }

            // If we don't match any `from`, we simply add a character
            res += &string[i..i + 1];
            i += 1;

            // Since we didn't apply anything transformation,
            // we reset the empty string replacement
            empty_string_replace = false;
        }

        // Special case when the string is empty or at the string's end
        // and one of the from is also empty
        for elem in std::iter::zip(from.iter(), to.iter()) {
            let from = elem.0.to_str()?;
            let to = elem.1.to_str()?;

            if from.as_str().is_empty() {
                res += &to;
                break;
            }
        }
        Ok(Value::String(res.into()))
    }

    #[builtin("seq")]
    async fn builtin_seq(co: GenCo, _x: Value, y: Value) -> Result<Value, ErrorKind> {
        // The builtin calling infra has already forced both args for us, so
        // we just return the second and ignore the first
        Ok(y)
    }

    #[builtin("split")]
    async fn builtin_split(co: GenCo, regex: Value, str: Value) -> Result<Value, ErrorKind> {
        let s = str.to_str()?;
        let text = s.as_str();
        let re = regex.to_str()?;
        let re: Regex = Regex::new(re.as_str()).unwrap();
        let mut capture_locations = re.capture_locations();
        let num_captures = capture_locations.len();
        let mut ret = imbl::Vector::new();
        let mut pos = 0;

        while let Some(thematch) = re.captures_read_at(&mut capture_locations, text, pos) {
            // push the unmatched characters preceding the match
            ret.push_back(Value::from(&text[pos..thematch.start()]));

            // Push a list with one element for each capture
            // group in the regex, containing the characters
            // matched by that capture group, or null if no match.
            // We skip capture 0; it represents the whole match.
            let v: imbl::Vector<Value> = (1..num_captures)
                .map(|i| capture_locations.get(i))
                .map(|o| {
                    o.map(|(start, end)| Value::from(&text[start..end]))
                        .unwrap_or(Value::Null)
                })
                .collect();
            ret.push_back(Value::List(NixList::from(v)));
            pos = thematch.end();
        }

        // push the unmatched characters following the last match
        ret.push_back(Value::from(&text[pos..]));

        Ok(Value::List(NixList::from(ret)))
    }

    #[builtin("sort")]
    async fn builtin_sort(co: GenCo, comparator: Value, list: Value) -> Result<Value, ErrorKind> {
        let list = list.to_list()?;
        let sorted = list.sort_by(&co, comparator).await?;
        Ok(Value::List(sorted))
    }

    #[builtin("splitVersion")]
    async fn builtin_split_version(co: GenCo, s: Value) -> Result<Value, ErrorKind> {
        let s = s.to_str()?;
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
    }

    #[builtin("stringLength")]
    async fn builtin_string_length(co: GenCo, #[lazy] s: Value) -> Result<Value, ErrorKind> {
        // also forces the value
        let s = s.coerce_to_string(co, CoercionKind::Weak).await?;
        Ok(Value::Integer(s.to_str()?.as_str().len() as i64))
    }

    #[builtin("sub")]
    async fn builtin_sub(co: GenCo, x: Value, y: Value) -> Result<Value, ErrorKind> {
        arithmetic_op!(&x, &y, -)
    }

    #[builtin("substring")]
    async fn builtin_substring(
        co: GenCo,
        start: Value,
        len: Value,
        s: Value,
    ) -> Result<Value, ErrorKind> {
        let beg = start.as_int()?;
        let len = len.as_int()?;
        let x = s.to_str()?;

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

        Ok(Value::String(x.as_str()[beg..end].into()))
    }

    #[builtin("tail")]
    async fn builtin_tail(co: GenCo, list: Value) -> Result<Value, ErrorKind> {
        let xs = list.to_list()?;

        if xs.is_empty() {
            Err(ErrorKind::TailEmptyList)
        } else {
            let output = xs.into_iter().skip(1).collect::<Vec<_>>();
            Ok(Value::List(NixList::construct(output.len(), output)))
        }
    }

    #[builtin("throw")]
    async fn builtin_throw(co: GenCo, message: Value) -> Result<Value, ErrorKind> {
        Err(ErrorKind::Throw(message.to_str()?.to_string()))
    }

    #[builtin("toString")]
    async fn builtin_to_string(co: GenCo, #[lazy] x: Value) -> Result<Value, ErrorKind> {
        // coerce_to_string forces for us
        x.coerce_to_string(co, CoercionKind::Strong).await
    }

    #[builtin("toXML")]
    async fn builtin_to_xml(co: GenCo, value: Value) -> Result<Value, ErrorKind> {
        let value = generators::request_deep_force(&co, value, SharedThunkSet::default()).await;
        let mut buf: Vec<u8> = vec![];
        to_xml::value_to_xml(&mut buf, &value)?;
        Ok(String::from_utf8(buf)?.into())
    }

    #[builtin("placeholder")]
    async fn builtin_placeholder(co: GenCo, #[lazy] _x: Value) -> Result<Value, ErrorKind> {
        generators::emit_warning(&co, WarningKind::NotImplemented("builtins.placeholder")).await;
        Ok("<builtins.placeholder-is-not-implemented-in-tvix-yet>".into())
    }

    #[builtin("trace")]
    async fn builtin_trace(co: GenCo, message: Value, value: Value) -> Result<Value, ErrorKind> {
        // TODO(grfn): `trace` should be pluggable and capturable, probably via a method on
        // the VM
        println!("trace: {} :: {}", message, message.type_of());
        Ok(value)
    }

    #[builtin("toPath")]
    async fn builtin_to_path(co: GenCo, s: Value) -> Result<Value, ErrorKind> {
        let path: Value = crate::value::canon_path(coerce_value_to_path(&co, s).await?).into();
        Ok(path.coerce_to_string(co, CoercionKind::Weak).await?)
    }

    #[builtin("tryEval")]
    async fn builtin_try_eval(co: GenCo, #[lazy] e: Value) -> Result<Value, ErrorKind> {
        let res = match generators::request_try_force(&co, e).await {
            Some(value) => [("value", value), ("success", true.into())],
            None => [("value", false.into()), ("success", false.into())],
        };

        Ok(Value::attrs(NixAttrs::from_iter(res.into_iter())))
    }

    #[builtin("typeOf")]
    async fn builtin_type_of(co: GenCo, x: Value) -> Result<Value, ErrorKind> {
        Ok(Value::String(x.type_of().into()))
    }
}

/// Internal helper function for genericClosure, determining whether a
/// value has been seen before.
async fn bgc_insert_key(co: &GenCo, key: Value, done: &mut Vec<Value>) -> Result<bool, ErrorKind> {
    for existing in done.iter() {
        if generators::check_equality(
            co,
            existing.clone(),
            key.clone(),
            // TODO(tazjin): not actually sure which semantics apply here
            PointerEquality::ForbidAll,
        )
        .await?
        {
            return Ok(false);
        }
    }

    done.push(key);
    Ok(true)
}

/// The set of standard pure builtins in Nix, mostly concerned with
/// data structure manipulation (string, attrs, list, etc. functions).
pub fn pure_builtins() -> Vec<(&'static str, Value)> {
    let mut result = pure_builtins::builtins();

    // Pure-value builtins
    result.push(("nixVersion", Value::String("2.3-compat-tvix-0.1".into())));
    result.push(("langVersion", Value::Integer(6)));
    result.push(("null", Value::Null));
    result.push(("true", Value::Bool(true)));
    result.push(("false", Value::Bool(false)));

    result.push((
        "currentSystem",
        crate::systems::llvm_triple_to_nix_double(CURRENT_PLATFORM).into(),
    ));

    result
}

#[builtins]
mod placeholder_builtins {
    use super::*;

    #[builtin("unsafeDiscardStringContext")]
    async fn builtin_unsafe_discard_string_context(
        _: GenCo,
        #[lazy] s: Value,
    ) -> Result<Value, ErrorKind> {
        // Tvix does not manually track contexts, and this is a no-op for us.
        Ok(s)
    }

    #[builtin("addErrorContext")]
    async fn builtin_add_error_context(
        co: GenCo,
        #[lazy] _context: Value,
        #[lazy] val: Value,
    ) -> Result<Value, ErrorKind> {
        generators::emit_warning(&co, WarningKind::NotImplemented("builtins.addErrorContext"))
            .await;
        Ok(val)
    }

    #[builtin("unsafeGetAttrPos")]
    async fn builtin_unsafe_get_attr_pos(
        co: GenCo,
        _name: Value,
        _attrset: Value,
    ) -> Result<Value, ErrorKind> {
        generators::emit_warning(
            &co,
            WarningKind::NotImplemented("builtins.unsafeGetAttrsPos"),
        )
        .await;
        let res = [
            ("line", 42.into()),
            ("col", 42.into()),
            ("file", Value::Path(Box::new("/deep/thought".into()))),
        ];
        Ok(Value::attrs(NixAttrs::from_iter(res.into_iter())))
    }
}

pub fn placeholders() -> Vec<(&'static str, Value)> {
    placeholder_builtins::builtins()
}
