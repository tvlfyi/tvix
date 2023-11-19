use crate::nixbase32;
use crate::nixhash::{self, HashAlgo, NixHash};
use serde::de::Unexpected;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::borrow::Cow;

use super::algos::SUPPORTED_ALGOS;
use super::from_algo_and_digest;

/// A Nix CAHash describes a content-addressed hash of a path.
///
/// The way Nix prints it as a string is a bit confusing, but there's essentially
/// three modes, `Flat`, `Nar` and `Text`.
/// `Flat` and `Nar` support all 4 algos that [NixHash] supports
/// (sha1, md5, sha256, sha512), `Text` only supports sha256.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CAHash {
    Flat(NixHash),  // "fixed flat"
    Nar(NixHash),   // "fixed recursive"
    Text([u8; 32]), // "text", only supports sha256
}

impl CAHash {
    pub fn digest(&self) -> Cow<NixHash> {
        match *self {
            CAHash::Flat(ref digest) => Cow::Borrowed(digest),
            CAHash::Nar(ref digest) => Cow::Borrowed(digest),
            CAHash::Text(digest) => Cow::Owned(NixHash::Sha256(digest)),
        }
    }

    /// Constructs a [CAHash] from the textual representation,
    /// which is one of the three:
    /// - `text:sha256:$nixbase32sha256digest`
    /// - `fixed:r:$algo:$nixbase32digest`
    /// - `fixed:$algo:$nixbase32digest`
    /// which is the format that's used in the NARInfo for example.
    pub fn from_nix_hex_str(s: &str) -> Option<Self> {
        let (tag, s) = s.split_once(':')?;

        match tag {
            "text" => {
                let digest = s.strip_prefix("sha256:")?;
                let digest = nixbase32::decode_fixed(digest).ok()?;
                Some(CAHash::Text(digest))
            }
            "fixed" => {
                if let Some(s) = s.strip_prefix("r:") {
                    NixHash::from_nix_hex_str(s).map(CAHash::Nar)
                } else {
                    NixHash::from_nix_hex_str(s).map(CAHash::Flat)
                }
            }
            _ => None,
        }
    }

    /// Formats a [CAHash] in the Nix default hash format, which is the format
    /// that's used in NARInfos for example.
    pub fn to_nix_nixbase32_string(&self) -> String {
        match self {
            CAHash::Flat(nh) => format!("fixed:{}", nh.to_nix_nixbase32_string()),
            CAHash::Nar(nh) => format!("fixed:r:{}", nh.to_nix_nixbase32_string()),
            CAHash::Text(digest) => {
                format!("text:sha256:{}", nixbase32::encode(digest))
            }
        }
    }

    /// This takes a serde_json::Map and turns it into this structure. This is necessary to do such
    /// shenigans because we have external consumers, like the Derivation parser, who would like to
    /// know whether we have a invalid or a missing NixHashWithMode structure in another structure,
    /// e.g. Output.
    /// This means we have this combinatorial situation:
    /// - no hash, no hashAlgo: no [CAHash] so we return Ok(None).
    /// - present hash, missing hashAlgo: invalid, we will return missing_field
    /// - missing hash, present hashAlgo: same
    /// - present hash, present hashAlgo: either we return ourselves or a type/value validation
    /// error.
    /// This function is for internal consumption regarding those needs until we have a better
    /// solution. Now this is said, let's explain how this works.
    ///
    /// We want to map the serde data model into a [CAHash].
    ///
    /// The serde data model has a `hash` field (containing a digest in nixbase32),
    /// and a `hashAlgo` field, containing the stringified hash algo.
    /// In case the hash is recursive, hashAlgo also has a `r:` prefix.
    ///
    /// This is to match how `nix show-derivation` command shows them in JSON
    /// representation.
    pub(crate) fn from_map<'de, D>(map: &Map<String, Value>) -> Result<Option<Self>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // If we don't have hash neither hashAlgo, let's just return None.
        if !map.contains_key("hash") && !map.contains_key("hashAlgo") {
            return Ok(None);
        }

        let digest: Vec<u8> = {
            if let Some(v) = map.get("hash") {
                if let Some(s) = v.as_str() {
                    data_encoding::HEXLOWER
                        .decode(s.as_bytes())
                        .map_err(|e| serde::de::Error::custom(e.to_string()))?
                } else {
                    return Err(serde::de::Error::invalid_type(
                        Unexpected::Other(&v.to_string()),
                        &"a string",
                    ));
                }
            } else {
                return Err(serde::de::Error::missing_field(
                    "couldn't extract `hash` key but `hashAlgo` key present",
                ));
            }
        };

        if let Some(v) = map.get("hashAlgo") {
            if let Some(s) = v.as_str() {
                match s.strip_prefix("r:") {
                    Some(rest) => Ok(Some(Self::Nar(
                        from_algo_and_digest(
                            HashAlgo::try_from(rest).map_err(|e| {
                                serde::de::Error::invalid_value(
                                    Unexpected::Other(&e.to_string()),
                                    &format!("one of {}", SUPPORTED_ALGOS.join(",")).as_str(),
                                )
                            })?,
                            &digest,
                        )
                        .map_err(|e: nixhash::Error| {
                            serde::de::Error::invalid_value(
                                Unexpected::Other(&e.to_string()),
                                &"a digest with right length",
                            )
                        })?,
                    ))),
                    None => Ok(Some(Self::Flat(
                        from_algo_and_digest(
                            HashAlgo::try_from(s).map_err(|e| {
                                serde::de::Error::invalid_value(
                                    Unexpected::Other(&e.to_string()),
                                    &format!("one of {}", SUPPORTED_ALGOS.join(",")).as_str(),
                                )
                            })?,
                            &digest,
                        )
                        .map_err(|e: nixhash::Error| {
                            serde::de::Error::invalid_value(
                                Unexpected::Other(&e.to_string()),
                                &"a digest with right length",
                            )
                        })?,
                    ))),
                }
            } else {
                Err(serde::de::Error::invalid_type(
                    Unexpected::Other(&v.to_string()),
                    &"a string",
                ))
            }
        } else {
            Err(serde::de::Error::missing_field(
                "couldn't extract `hashAlgo` key, but `hash` key present",
            ))
        }
    }
}

impl Serialize for CAHash {
    /// map a CAHash into the serde data model.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            CAHash::Flat(h) => {
                map.serialize_entry("hash", &nixbase32::encode(h.digest_as_bytes()))?;
                map.serialize_entry("hashAlgo", &h.algo())?;
            }
            CAHash::Nar(h) => {
                map.serialize_entry("hash", &nixbase32::encode(h.digest_as_bytes()))?;
                map.serialize_entry("hashAlgo", &format!("r:{}", &h.algo()))?;
            }
            // It is not legal for derivations to use this (which is where
            // we're currently exercising [Serialize] mostly,
            // but it's still good to be able to serialize other CA hashes too.
            CAHash::Text(h) => {
                map.serialize_entry("hash", &nixbase32::encode(h.as_ref()))?;
                map.serialize_entry("hashAlgo", "text")?;
            }
        };
        map.end()
    }
}

impl<'de> Deserialize<'de> for CAHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Self::from_map::<D>(&Map::deserialize(deserializer)?)?;

        match value {
            None => Err(serde::de::Error::custom("couldn't parse as map")),
            Some(v) => Ok(v),
        }
    }
}
