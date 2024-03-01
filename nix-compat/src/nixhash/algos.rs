use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::nixhash::Error;

/// This are the hash algorithms supported by cppnix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HashAlgo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
}

impl HashAlgo {
    // return the number of bytes in the digest of the given hash algo.
    pub fn digest_length(&self) -> usize {
        match self {
            HashAlgo::Sha1 => 20,
            HashAlgo::Sha256 => 32,
            HashAlgo::Sha512 => 64,
            HashAlgo::Md5 => 16,
        }
    }
}

impl Display for HashAlgo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            HashAlgo::Md5 => write!(f, "md5"),
            HashAlgo::Sha1 => write!(f, "sha1"),
            HashAlgo::Sha256 => write!(f, "sha256"),
            HashAlgo::Sha512 => write!(f, "sha512"),
        }
    }
}

impl Serialize for HashAlgo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_str(&self)
    }
}

impl<'de> Deserialize<'de> for HashAlgo {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s: &str = Deserialize::deserialize(deserializer)?;
        HashAlgo::try_from(s).map_err(serde::de::Error::custom)
    }
}

/// TODO(Raito): this could be automated via macros, I suppose.
/// But this may be more expensive than just doing it by hand
/// and ensuring that is kept in sync.
pub const SUPPORTED_ALGOS: [&str; 4] = ["md5", "sha1", "sha256", "sha512"];

impl TryFrom<&str> for HashAlgo {
    type Error = Error;

    fn try_from(algo_str: &str) -> Result<Self, Self::Error> {
        match algo_str {
            "md5" => Ok(Self::Md5),
            "sha1" => Ok(Self::Sha1),
            "sha256" => Ok(Self::Sha256),
            "sha512" => Ok(Self::Sha512),
            _ => Err(Error::InvalidAlgo(algo_str.to_string())),
        }
    }
}
