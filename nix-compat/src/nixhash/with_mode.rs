use crate::nixbase32;
use crate::nixhash::{HashAlgo, NixHash};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A Nix Hash can either be flat or recursive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NixHashWithMode {
    Flat(NixHash),
    Recursive(NixHash),
}

impl NixHashWithMode {
    /// Formats a [NixHashWithMode] in the Nix default hash format,
    /// which is the algo, followed by a colon, then the lower hex encoded digest.
    /// In case the hash itself is recursive, a `r:` is added as prefix
    pub fn to_nix_hash_string(&self) -> String {
        match self {
            NixHashWithMode::Flat(h) => h.to_nix_hash_string(),
            NixHashWithMode::Recursive(h) => format!("r:{}", h.to_nix_hash_string()),
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
                map.serialize_entry("hash", &nixbase32::encode(&h.digest))?;
                map.serialize_entry("hashAlgo", &h.algo.to_string())?;
            }
            NixHashWithMode::Recursive(h) => {
                map.serialize_entry("hash", &nixbase32::encode(&h.digest))?;
                map.serialize_entry("hashAlgo", &format!("r:{}", &h.algo.to_string()))?;
            }
        };
        map.end()
    }
}

impl<'de> Deserialize<'de> for NixHashWithMode {
    /// map the serde data model into a NixHashWithMode.
    ///
    /// The serde data model has a `hash` field (containing a digest in nixbase32),
    /// and a `hashAlgo` field, containing the stringified hash algo.
    /// In case the hash is recursive, hashAlgo also has a `r:` prefix.
    ///
    /// This is to match how `nix show-derivation` command shows them in JSON
    /// representation.
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // TODO: don't use serde_json here?
        // TODO: serde seems to simply set `hash_with_mode` to None if hash
        // and hashAlgo fail, but that should be a proper deserialization error
        // that should be propagated to the user!

        let json = serde_json::Value::deserialize(deserializer)?;
        match json.as_object() {
            None => Err(serde::de::Error::custom("couldn't parse as map"))?,
            Some(map) => {
                let digest: Vec<u8> = {
                    if let Some(v) = map.get("hash") {
                        if let Some(s) = v.as_str() {
                            data_encoding::HEXLOWER
                                .decode(s.as_bytes())
                                .map_err(|e| serde::de::Error::custom(e.to_string()))?
                        } else {
                            return Err(serde::de::Error::custom(
                                "couldn't parse 'hash' as string",
                            ));
                        }
                    } else {
                        return Err(serde::de::Error::custom("couldn't extract 'hash' key"));
                    }
                };

                if let Some(v) = map.get("hashAlgo") {
                    if let Some(s) = v.as_str() {
                        match s.strip_prefix("r:") {
                            Some(rest) => Ok(NixHashWithMode::Recursive(NixHash::new(
                                HashAlgo::try_from(rest).map_err(|e| {
                                    serde::de::Error::custom(format!("unable to parse algo: {}", e))
                                })?,
                                digest,
                            ))),
                            None => Ok(NixHashWithMode::Flat(NixHash::new(
                                HashAlgo::try_from(s).map_err(|e| {
                                    serde::de::Error::custom(format!("unable to parse algo: {}", e))
                                })?,
                                digest,
                            ))),
                        }
                    } else {
                        Err(serde::de::Error::custom(
                            "couldn't parse 'hashAlgo' as string",
                        ))
                    }
                } else {
                    Err(serde::de::Error::custom("couldn't extract 'hashAlgo' key"))
                }
            }
        }
    }
}
