/// Implementation of Value serialisation *to* JSON.
///
/// This can not be implemented through standard serde-derive methods,
/// as there is internal Nix logic that must happen within the
/// serialisation methods.
use super::{CoercionKind, Value};
use crate::generators::{self, GenCo};
use crate::ErrorKind;

use serde_json::value::to_value;
use serde_json::Value as Json;
use serde_json::{Map, Number}; // name clash with *our* `Value`

impl Value {
    pub(crate) async fn to_json(self, co: &GenCo) -> Result<Json, ErrorKind> {
        let self_forced = generators::request_force(co, self).await;

        let value = match self_forced {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(b),
            Value::Integer(i) => Json::Number(Number::from(i)),
            Value::Float(f) => to_value(f)?,
            Value::String(s) => Json::String(s.as_str().into()),

            Value::Path(p) => {
                let imported = generators::request_path_import(co, *p).await;
                Json::String(imported.to_string_lossy().to_string())
            }

            Value::List(l) => {
                let mut out = vec![];

                for val in l.into_iter() {
                    out.push(generators::request_to_json(co, val).await);
                }

                Json::Array(out)
            }

            Value::Attrs(attrs) => {
                // Attribute sets with a callable `__toString` attribute
                // serialise to the string-coerced version of the result of
                // calling that.
                if let Some(s) = attrs.try_to_string(&co, CoercionKind::Weak).await {
                    return Ok(Json::String(s.as_str().to_string()));
                }

                // Attribute sets with an `outPath` attribute
                // serialise to a JSON serialisation of that inner
                // value (regardless of what it is!).
                if let Some(out_path) = attrs.select("outPath") {
                    return Ok(generators::request_to_json(co, out_path.clone()).await);
                }

                let mut out = Map::with_capacity(attrs.len());
                for (name, value) in attrs.into_iter_sorted() {
                    out.insert(
                        name.as_str().to_string(),
                        generators::request_to_json(co, value).await,
                    );
                }

                Json::Object(out)
            }

            val @ Value::Closure(_)
            | val @ Value::Thunk(_)
            | val @ Value::Builtin(_)
            | val @ Value::AttrNotFound
            | val @ Value::Blueprint(_)
            | val @ Value::DeferredUpvalue(_)
            | val @ Value::UnresolvedPath(_)
            | val @ Value::Json(_) => return Err(ErrorKind::NotSerialisableToJson(val.type_of())),
        };

        Ok(value)
    }

    /// Generator version of the above, which wraps responses in
    /// Value::Json.
    pub(crate) async fn to_json_generator(self, co: GenCo) -> Result<Value, ErrorKind> {
        Ok(Value::Json(self.to_json(&co).await?))
    }
}
