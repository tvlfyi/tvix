/// This module implements Nix attribute sets. They have flexible
/// backing implementations, as they are used in very versatile
/// use-cases that are all exposed the same way in the language
/// surface.
use std::collections::BTreeMap;
use std::fmt::Display;

use super::string::NixString;
use super::Value;

#[derive(Debug)]
pub enum NixAttrs {
    Map(BTreeMap<NixString, Value>),
    KV { name: NixString, value: Value },
}

impl Display for NixAttrs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("{ ")?;

        match self {
            NixAttrs::KV { name, value } => {
                f.write_fmt(format_args!("name = \"{}\"; ", name))?;
                f.write_fmt(format_args!("value = {}; ", value))?;
            }

            NixAttrs::Map(map) => {
                for (name, value) in map {
                    f.write_fmt(format_args!("{} = {}; ", name, value))?;
                }
            }
        }

        f.write_str("}")
    }
}

impl PartialEq for NixAttrs {
    fn eq(&self, _other: &Self) -> bool {
        todo!("attrset equality")
    }
}
