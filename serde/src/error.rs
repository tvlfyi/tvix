//! When serialising Nix goes wrong ...

use std::error;
use std::fmt::Display;

#[derive(Clone, Debug)]
pub enum Error {
    /// Attempted to deserialise an unsupported Nix value (such as a
    /// function) that can not be represented by the
    /// [`serde::Deserialize`] trait.
    Unserializable { value_type: &'static str },

    /// Expected to deserialize a value that is unsupported by Nix.
    Unsupported { wanted: &'static str },

    /// Expected a specific type, but got something else on the Nix side.
    UnexpectedType {
        expected: &'static str,
        got: &'static str,
    },

    /// Deserialisation error returned from `serde::de`.
    Deserialization(String),

    /// Deserialized integer did not fit.
    IntegerConversion { got: i64, need: &'static str },

    /// Evaluation of the supplied Nix code failed while computing the
    /// value for deserialisation.
    NixErrors { errors: Vec<tvix_eval::Error> },

    /// Could not determine an externally tagged enum representation.
    AmbiguousEnum,

    /// Attempted to provide content to a unit enum.
    UnitEnumContent,
}

impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unserializable { value_type } => write!(
                f,
                "can not deserialise a Nix '{}' into a Rust type",
                value_type
            ),

            Error::Unsupported { wanted } => {
                write!(f, "can not deserialize a '{}' from a Nix value", wanted)
            }

            Error::UnexpectedType { expected, got } => {
                write!(f, "expected type {}, but got Nix type {}", expected, got)
            }

            Error::NixErrors { errors } => {
                writeln!(
                    f,
                    "{} occured during Nix evaluation: ",
                    if errors.len() == 1 { "error" } else { "errors" }
                )?;

                for err in errors {
                    writeln!(f, "{}", err.fancy_format_str())?;
                }

                Ok(())
            }

            Error::Deserialization(err) => write!(f, "deserialisation error occured: {}", err),

            Error::IntegerConversion { got, need } => {
                write!(f, "i64({}) does not fit in a {}", got, need)
            }

            Error::AmbiguousEnum => write!(f, "could not determine enum variant: ambiguous keys"),

            Error::UnitEnumContent => write!(f, "provided content for unit enum variant"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::NixErrors { errors, .. } => errors.first().map(|e| e as &dyn error::Error),
            _ => None,
        }
    }
}

impl serde::de::Error for Error {
    fn custom<T>(err: T) -> Self
    where
        T: Display,
    {
        Self::Deserialization(err.to_string())
    }
}
