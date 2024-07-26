use bytes::Bytes;
use data_encoding::BASE64;
use thiserror::Error;

#[derive(PartialEq, Eq, Hash)]
pub struct B3Digest(Bytes);

// TODO: allow converting these errors to crate::Error
#[derive(Error, Debug)]
pub enum Error {
    #[error("invalid digest length: {0}")]
    InvalidDigestLen(usize),
}

pub const B3_LEN: usize = 32;

impl B3Digest {
    pub fn as_slice(&self) -> &[u8] {
        &self.0[..]
    }
}

impl From<B3Digest> for bytes::Bytes {
    fn from(val: B3Digest) -> Self {
        val.0
    }
}

impl From<blake3::Hash> for B3Digest {
    fn from(value: blake3::Hash) -> Self {
        Self(Bytes::copy_from_slice(value.as_bytes()))
    }
}
impl From<digest::Output<blake3::Hasher>> for B3Digest {
    fn from(value: digest::Output<blake3::Hasher>) -> Self {
        let v = Into::<[u8; B3_LEN]>::into(value);
        Self(Bytes::copy_from_slice(&v))
    }
}

impl TryFrom<Vec<u8>> for B3Digest {
    type Error = Error;

    // constructs a [B3Digest] from a [Vec<u8>].
    // Returns an error if the digest has the wrong length.
    fn try_from(value: Vec<u8>) -> Result<Self, Self::Error> {
        if value.len() != B3_LEN {
            Err(Error::InvalidDigestLen(value.len()))
        } else {
            Ok(Self(value.into()))
        }
    }
}

impl TryFrom<bytes::Bytes> for B3Digest {
    type Error = Error;

    // constructs a [B3Digest] from a [bytes::Bytes].
    // Returns an error if the digest has the wrong length.
    fn try_from(value: bytes::Bytes) -> Result<Self, Self::Error> {
        if value.len() != B3_LEN {
            Err(Error::InvalidDigestLen(value.len()))
        } else {
            Ok(Self(value))
        }
    }
}

impl From<&[u8; B3_LEN]> for B3Digest {
    fn from(value: &[u8; B3_LEN]) -> Self {
        Self(value.to_vec().into())
    }
}

impl From<B3Digest> for [u8; B3_LEN] {
    fn from(value: B3Digest) -> Self {
        value.0.to_vec().try_into().unwrap()
    }
}

impl Clone for B3Digest {
    fn clone(&self) -> Self {
        Self(self.0.to_owned())
    }
}

impl std::fmt::Display for B3Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "b3:{}", BASE64.encode(&self.0))
    }
}

impl std::fmt::Debug for B3Digest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "b3:{}", BASE64.encode(&self.0))
    }
}
