//! Deserialisation from Nix to Rust values.

use serde::de::value::{MapDeserializer, SeqDeserializer};
use serde::de::{self, EnumAccess, VariantAccess};
use tvix_eval::Value;

use crate::error::Error;

struct NixDeserializer {
    value: tvix_eval::Value,
}

impl NixDeserializer {
    fn new(value: Value) -> Self {
        if let Value::Thunk(thunk) = value {
            Self::new(thunk.value().clone())
        } else {
            Self { value }
        }
    }
}

impl de::IntoDeserializer<'_, Error> for NixDeserializer {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self::Deserializer {
        self
    }
}

pub fn from_str<'code, T>(src: &'code str) -> Result<T, Error>
where
    T: serde::Deserialize<'code>,
{
    // First step is to evaluate the Nix code ...
    let mut eval = tvix_eval::Evaluation::new(src, None);
    eval.strict = true;
    let source = eval.source_map();
    let result = eval.evaluate();

    if !result.errors.is_empty() {
        return Err(Error::NixErrors {
            errors: result.errors,
            source,
        });
    }

    let de = NixDeserializer::new(result.value.expect("value should be present on success"));

    T::deserialize(de)
}

fn unexpected(expected: &'static str, got: &Value) -> Error {
    Error::UnexpectedType {
        expected,
        got: got.type_of(),
    }
}

fn visit_integer<I: TryFrom<i64>>(v: &Value) -> Result<I, Error> {
    match v {
        Value::Integer(i) => I::try_from(*i).map_err(|_| Error::IntegerConversion {
            got: *i,
            need: std::any::type_name::<I>(),
        }),

        _ => Err(unexpected("integer", v)),
    }
}

impl<'de> de::Deserializer<'de> for NixDeserializer {
    type Error = Error;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Null => visitor.visit_unit(),
            Value::Bool(b) => visitor.visit_bool(b),
            Value::Integer(i) => visitor.visit_i64(i),
            Value::Float(f) => visitor.visit_f64(f),
            Value::String(s) => visitor.visit_string(s.to_string()),
            Value::Path(p) => visitor.visit_string(p.to_string_lossy().into()), // TODO: hmm
            Value::Attrs(_) => self.deserialize_map(visitor),
            Value::List(_) => self.deserialize_seq(visitor),

            // tvix-eval types that can not be deserialized through serde.
            Value::Closure(_)
            | Value::Builtin(_)
            | Value::Thunk(_)
            | Value::AttrNotFound
            | Value::Blueprint(_)
            | Value::DeferredUpvalue(_)
            | Value::UnresolvedPath(_)
            | Value::Json(_) => Err(Error::Unserializable {
                value_type: self.value.type_of(),
            }),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            Value::Bool(b) => visitor.visit_bool(b),
            _ => Err(unexpected("bool", &self.value)),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i8(visit_integer(&self.value)?)
    }

    fn deserialize_i16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i16(visit_integer(&self.value)?)
    }

    fn deserialize_i32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i32(visit_integer(&self.value)?)
    }

    fn deserialize_i64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_i64(visit_integer(&self.value)?)
    }

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u8(visit_integer(&self.value)?)
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u16(visit_integer(&self.value)?)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u32(visit_integer(&self.value)?)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_u64(visit_integer(&self.value)?)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::Float(f) = self.value {
            return visitor.visit_f32(f as f32);
        }

        Err(unexpected("float", &self.value))
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::Float(f) = self.value {
            return visitor.visit_f64(f);
        }

        Err(unexpected("float", &self.value))
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::String(s) = &self.value {
            let chars = s.as_str().chars().collect::<Vec<_>>();
            if chars.len() == 1 {
                return visitor.visit_char(chars[0]);
            }
        }

        Err(unexpected("char", &self.value))
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::String(s) = &self.value {
            return visitor.visit_str(s.as_str());
        }

        Err(unexpected("string", &self.value))
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::String(s) = &self.value {
            return visitor.visit_str(s.as_str());
        }

        Err(unexpected("string", &self.value))
    }

    fn deserialize_bytes<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unimplemented!()
    }

    fn deserialize_byte_buf<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        unimplemented!()
    }

    // Note that this can not distinguish between a serialisation of
    // `Some(())` and `None`.
    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::Null = self.value {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::Null = self.value {
            return visitor.visit_unit();
        }

        Err(unexpected("null", &self.value))
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::List(list) = self.value {
            let mut seq =
                SeqDeserializer::new(list.into_iter().map(|value| NixDeserializer::new(value)));
            let result = visitor.visit_seq(&mut seq)?;
            seq.end()?;
            return Ok(result);
        }

        Err(unexpected("list", &self.value))
    }

    fn deserialize_tuple<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        // just represent tuples as lists ...
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        // same as above
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        if let Value::Attrs(attrs) = self.value {
            let mut map = MapDeserializer::new(attrs.into_iter().map(|(k, v)| {
                (
                    NixDeserializer::new(Value::String(k)),
                    NixDeserializer::new(v),
                )
            }));
            let result = visitor.visit_map(&mut map)?;
            map.end()?;
            return Ok(result);
        }

        Err(unexpected("map", &self.value))
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    // This method is responsible for deserializing the externally
    // tagged enum variant serialisation.
    fn deserialize_enum<V>(
        self,
        name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        match self.value {
            // a string represents a unit variant
            Value::String(s) => visitor.visit_enum(de::value::StrDeserializer::new(s.as_str())),

            // an attribute set however represents an externally
            // tagged enum with content
            Value::Attrs(attrs) => visitor.visit_enum(Enum(*attrs)),

            _ => Err(unexpected(name, &self.value)),
        }
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }
}

struct Enum(tvix_eval::NixAttrs);

impl<'de> EnumAccess<'de> for Enum {
    type Error = Error;
    type Variant = NixDeserializer;

    // TODO: pass the known variants down here and check against them
    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: de::DeserializeSeed<'de>,
    {
        if self.0.len() != 1 {
            return Err(Error::AmbiguousEnum);
        }

        let (key, value) = self.0.into_iter().next().expect("length asserted above");
        let val = seed.deserialize(de::value::StrDeserializer::<Error>::new(key.as_str()))?;

        Ok((val, NixDeserializer::new(value)))
    }
}

impl<'de> VariantAccess<'de> for NixDeserializer {
    type Error = Error;

    fn unit_variant(self) -> Result<(), Self::Error> {
        // If this case is hit, a user specified the name of a unit
        // enum variant but gave it content. Unit enum deserialisation
        // is handled in `deserialize_enum` above.
        Err(Error::UnitEnumContent)
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: de::DeserializeSeed<'de>,
    {
        seed.deserialize(self)
    }

    fn tuple_variant<V>(self, _len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_seq(self, visitor)
    }

    fn struct_variant<V>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        de::Deserializer::deserialize_map(self, visitor)
    }
}
