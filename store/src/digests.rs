use data_encoding::BASE64;
use thiserror::Error;

// FUTUREWORK: make generic

#[derive(PartialEq, Eq, Hash, Debug)]
pub struct B3Digest(Vec<u8>);

// TODO: allow converting these errors to crate::Error
#[derive(Error, Debug)]
pub enum Error {
    #[error("invalid digest length: {0}")]
    InvalidDigestLen(usize),
}

impl B3Digest {
    // constructs a [B3Digest] from a [Vec<u8>].
    // Returns an error if the digest has the wrong length.
    pub fn from_vec(value: Vec<u8>) -> Result<Self, Error> {
        if value.len() != 32 {
            Err(Error::InvalidDigestLen(value.len()))
        } else {
            Ok(Self(value))
        }
    }

    // returns a copy of the inner [Vec<u8>].
    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}

impl From<&[u8; 32]> for B3Digest {
    fn from(value: &[u8; 32]) -> Self {
        Self(value.to_vec())
    }
}

impl Clone for B3Digest {
    fn clone(&self) -> Self {
        Self(self.0.to_owned())
    }
}

impl std::fmt::Display for B3Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "b3:{}", BASE64.encode(self.0.as_slice()))
    }
}
