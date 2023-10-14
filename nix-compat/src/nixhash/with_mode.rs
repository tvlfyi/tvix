use crate::nixbase32;
use crate::nixhash::{self, HashAlgo, NixHash};
use serde::de::Unexpected;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::algos::SUPPORTED_ALGOS;
use super::from_algo_and_digest;

pub enum NixHashMode {
    Flat,
    Recursive,
}

impl NixHashMode {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::Flat => "",
            Self::Recursive => "r:",
        }
    }
}

/// A Nix Hash can either be flat or recursive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NixHashWithMode {
    Flat(NixHash),
    Recursive(NixHash),
}

impl NixHashWithMode {
    /// Construct a [NixHashWithMode] from a string containing the algo, and
    /// optionally a `r:` prefix, and a digest (bytes).
    pub fn from_algo_mode_hash(algo_and_mode: &str, digest: &[u8]) -> super::Result<Self> {
        Ok(match algo_and_mode.strip_prefix("r:") {
            Some(algo) => nixhash::NixHashWithMode::Recursive(nixhash::from_algo_and_digest(
                algo.try_into()?,
                &digest,
            )?),
            None => nixhash::NixHashWithMode::Flat(nixhash::from_algo_and_digest(
                algo_and_mode.try_into()?,
                &digest,
            )?),
        })
    }

    pub fn mode(&self) -> NixHashMode {
        match self {
            Self::Flat(_) => NixHashMode::Flat,
            Self::Recursive(_) => NixHashMode::Recursive,
        }
    }

    pub fn digest(&self) -> &NixHash {
        match self {
            Self::Flat(ref h) => h,
            Self::Recursive(ref h) => h,
        }
    }

    /// Formats a [NixHashWithMode] in the Nix default hash format,
    /// which is the algo, followed by a colon, then the lower hex encoded digest.
    /// In case the hash itself is recursive, a `r:` is added as prefix
    pub fn to_nix_hash_string(&self) -> String {
        String::from(self.mode().prefix()) + &self.digest().to_nix_hash_string()
    }

    /// This takes a serde_json::Map and turns it into this structure. This is necessary to do such
    /// shenigans because we have external consumers, like the Derivation parser, who would like to
    /// know whether we have a invalid or a missing NixHashWithMode structure in another structure,
    /// e.g. Output.
    /// This means we have this combinatorial situation:
    /// - no hash, no hashAlgo: no NixHashWithMode so we return Ok(None).
    /// - present hash, missing hashAlgo: invalid, we will return missing_field
    /// - missing hash, present hashAlgo: same
    /// - present hash, present hashAlgo: either we return ourselves or a type/value validation
    /// error.
    /// This function is for internal consumption regarding those needs until we have a better
    /// solution. Now this is said, let's explain how this works.
    ///
    /// We want to map the serde data model into a NixHashWithMode.
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
                    Some(rest) => Ok(Some(Self::Recursive(
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

impl Serialize for NixHashWithMode {
    /// map a NixHashWithMode into the serde data model.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(2))?;
        match self {
            NixHashWithMode::Flat(h) => {
                map.serialize_entry("hash", &nixbase32::encode(h.digest_as_bytes()))?;
                map.serialize_entry("hashAlgo", &h.algo())?;
            }
            NixHashWithMode::Recursive(h) => {
                map.serialize_entry("hash", &nixbase32::encode(h.digest_as_bytes()))?;
                map.serialize_entry("hashAlgo", &format!("r:{}", &h.algo()))?;
            }
        };
        map.end()
    }
}

impl<'de> Deserialize<'de> for NixHashWithMode {
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

#[cfg(test)]
mod tests {
    use crate::nixhash::{NixHash, NixHashWithMode};
    use lazy_static::lazy_static;
    use test_case::test_case;

    const DIGEST_SHA256: [u8; 32] = [
        0xa5, 0xce, 0x9c, 0x15, 0x5e, 0xd0, 0x93, 0x97, 0x61, 0x46, 0x46, 0xc9, 0x71, 0x7f, 0xc7,
        0xcd, 0x94, 0xb1, 0x02, 0x3d, 0x7b, 0x76, 0xb6, 0x18, 0xd4, 0x09, 0xe4, 0xfe, 0xfd, 0x6e,
        0x9d, 0x39,
    ];
    lazy_static! {
        pub static ref NIXHASH_SHA256: NixHash = NixHash::Sha256(DIGEST_SHA256);
    }

    #[test_case("sha256", &DIGEST_SHA256, NixHashWithMode::Flat(NIXHASH_SHA256.clone()); "sha256 flat")]
    #[test_case("r:sha256", &DIGEST_SHA256, NixHashWithMode::Recursive(NIXHASH_SHA256.clone()); "sha256 recursive")]
    fn from_from_algo_mode_hash(algo_and_mode: &str, digest: &[u8], expected: NixHashWithMode) {
        assert_eq!(
            expected,
            NixHashWithMode::from_algo_mode_hash(algo_and_mode, digest).unwrap()
        );
    }

    #[test]
    fn from_algo_mode_failure() {
        assert!(NixHashWithMode::from_algo_mode_hash("r:sha256", &[]).is_err());
        assert!(NixHashWithMode::from_algo_mode_hash("ha256", &DIGEST_SHA256).is_err());
    }
}
