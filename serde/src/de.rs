//! Deserialisation from Nix to Rust values.

use serde::de;
use tvix_eval::Value;

use crate::error::Error;

struct Deserializer {
    value: tvix_eval::Value,
}

pub fn from_str<'code, T>(src: &'code str) -> Result<T, Error>
where
    T: serde::Deserialize<'code>,
{
    // First step is to evaluate the Nix code ...
    let eval = tvix_eval::Evaluation::new(src, None);
    let source = eval.source_map();
    let result = eval.evaluate();

    if !result.errors.is_empty() {
        return Err(Error::NixErrors {
            errors: result.errors,
            source,
        });
    }

    let de = Deserializer {
        value: result.value.expect("value should be present on success"),
    };

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

impl<'de> de::Deserializer<'de> for Deserializer {
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
            | Value::UnresolvedPath(_) => Err(Error::Unserializable {
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

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!("how to represent this?");
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!("how to represent this?");
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!("how to represent this?");
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
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!("how to represent this?");
    }

    fn deserialize_newtype_struct<V>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!("how to represent this?");
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_tuple_struct<V>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_struct<V>(
        self,
        name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_enum<V>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        todo!()
    }

    fn deserialize_ignored_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: de::Visitor<'de>,
    {
        visitor.visit_unit()
    }
}
