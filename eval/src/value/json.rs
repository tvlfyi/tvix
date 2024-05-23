/// Implementation of Value serialisation *to* JSON.
///
/// This can not be implemented through standard serde-derive methods,
/// as there is internal Nix logic that must happen within the
/// serialisation methods.
use super::{CoercionKind, Value};
use crate::errors::{CatchableErrorKind, ErrorKind};
use crate::generators::{self, GenCo};
use crate::NixContext;

use bstr::ByteSlice;
use serde_json::value::to_value;
use serde_json::Value as Json; // name clash with *our* `Value`
use serde_json::{Map, Number};

impl Value {
    /// Transforms the structure into a JSON
    /// and accumulate all encountered context in the second's element
    /// of the return type.
    pub async fn into_contextful_json(
        self,
        co: &GenCo,
    ) -> Result<Result<(Json, NixContext), CatchableErrorKind>, ErrorKind> {
        let self_forced = generators::request_force(co, self).await;
        let mut context = NixContext::new();

        let value = match self_forced {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(b),
            Value::Integer(i) => Json::Number(Number::from(i)),
            Value::Float(f) => to_value(f)?,
            Value::String(s) => {
                context.mimic(&s);

                Json::String(s.to_str()?.to_owned())
            }

            Value::Path(p) => {
                let imported = generators::request_path_import(co, *p).await;
                let path = imported.to_string_lossy().to_string();
                context = context.append(crate::NixContextElement::Plain(path.clone()));
                Json::String(path)
            }

            Value::List(l) => {
                let mut out = vec![];

                for val in l.into_iter() {
                    match generators::request_to_json(co, val).await {
                        Ok((v, ctx)) => {
                            context.extend(ctx.into_iter());
                            out.push(v)
                        }
                        Err(cek) => return Ok(Err(cek)),
                    }
                }

                Json::Array(out)
            }

            Value::Attrs(attrs) => {
                // Attribute sets with a callable `__toString` attribute
                // serialise to the string-coerced version of the result of
                // calling that.
                if attrs.select("__toString").is_some() {
                    let span = generators::request_span(co).await;
                    match Value::Attrs(attrs)
                        .coerce_to_string_(
                            co,
                            CoercionKind {
                                strong: false,
                                import_paths: false,
                            },
                            span,
                        )
                        .await?
                    {
                        Value::Catchable(cek) => return Ok(Err(*cek)),
                        Value::String(s) => {
                            // We need a fresh context here because `__toString` will discard
                            // everything.
                            let mut fresh = NixContext::new();
                            fresh.mimic(&s);

                            return Ok(Ok((Json::String(s.to_str()?.to_owned()), fresh)));
                        }
                        _ => panic!("Value::coerce_to_string_() returned a non-string!"),
                    }
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
                        name.to_str()?.to_owned(),
                        match generators::request_to_json(co, value).await {
                            Ok((v, ctx)) => {
                                context.extend(ctx.into_iter());
                                v
                            }
                            Err(cek) => return Ok(Err(cek)),
                        },
                    );
                }

                Json::Object(out)
            }

            Value::Catchable(c) => return Ok(Err(*c)),

            val @ Value::Closure(_)
            | val @ Value::Thunk(_)
            | val @ Value::Builtin(_)
            | val @ Value::AttrNotFound
            | val @ Value::Blueprint(_)
            | val @ Value::DeferredUpvalue(_)
            | val @ Value::UnresolvedPath(_)
            | val @ Value::Json(..)
            | val @ Value::FinaliseRequest(_) => {
                return Err(ErrorKind::NotSerialisableToJson(val.type_of()))
            }
        };

        Ok(Ok((value, context)))
    }

    /// Generator version of the above, which wraps responses in
    /// [`Value::Json`].
    pub(crate) async fn into_contextful_json_generator(
        self,
        co: GenCo,
    ) -> Result<Value, ErrorKind> {
        match self.into_contextful_json(&co).await? {
            Err(cek) => Ok(Value::from(cek)),
            Ok((json, ctx)) => Ok(Value::Json(Box::new((json, ctx)))),
        }
    }

    /// Transforms the structure into a JSON
    /// All the accumulated context is ignored, use [`into_contextful_json`]
    /// to obtain the resulting context of the JSON object.
    pub async fn into_json(
        self,
        co: &GenCo,
    ) -> Result<Result<Json, CatchableErrorKind>, ErrorKind> {
        Ok(self.into_contextful_json(co).await?.map(|(json, _)| json))
    }
}
