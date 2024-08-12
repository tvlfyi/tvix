//! Parser for the Nix archive listing format, aka .ls.
//!
//! LS files are produced by the C++ Nix implementation via `write-nar-listing=1` query parameter
//! passed to a store implementation when transferring store paths.
//!
//! Listing files contains metadata about a file and its offset in the corresponding NAR.
//!
//! NOTE: LS entries does not offer any integrity field to validate the retrieved file at the provided
//! offset. Validating the contents is the caller's responsibility.

use std::collections::HashMap;

use serde::Deserialize;

#[cfg(test)]
mod test;

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ListingEntry {
    Regular {
        size: u64,
        #[serde(default)]
        executable: bool,
        #[serde(rename = "narOffset")]
        nar_offset: u64,
    },
    Directory {
        entries: HashMap<String, ListingEntry>,
    },
    Symlink {
        target: String,
    },
}

#[derive(Debug)]
pub struct ListingVersion<const V: u8>;

#[derive(Debug, thiserror::Error)]
#[error("Invalid version: {0}")]
struct ListingVersionError(u8);

impl<'de, const V: u8> Deserialize<'de> for ListingVersion<V> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        if value == V {
            Ok(ListingVersion::<V>)
        } else {
            Err(serde::de::Error::custom(ListingVersionError(value)))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
#[non_exhaustive]
pub enum Listing {
    V1 {
        root: ListingEntry,
        version: ListingVersion<1>,
    },
}
