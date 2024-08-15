// TODO: split out this error
use crate::ValidateNodeError;

use bstr::ByteSlice;
use std::fmt::{self, Debug, Display};

/// A wrapper type for symlink targets.
/// Internally uses a [bytes::Bytes], but disallows empty targets and those
/// containing null bytes.
#[repr(transparent)]
#[derive(Clone, PartialEq, Eq)]
pub struct SymlinkTarget {
    inner: bytes::Bytes,
}

impl AsRef<[u8]> for SymlinkTarget {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_ref()
    }
}

impl From<SymlinkTarget> for bytes::Bytes {
    fn from(value: SymlinkTarget) -> Self {
        value.inner
    }
}

impl TryFrom<bytes::Bytes> for SymlinkTarget {
    type Error = ValidateNodeError;

    fn try_from(value: bytes::Bytes) -> Result<Self, Self::Error> {
        if value.is_empty() || value.contains(&b'\0') {
            return Err(ValidateNodeError::InvalidSymlinkTarget(value));
        }

        Ok(Self { inner: value })
    }
}

impl TryFrom<&'static [u8]> for SymlinkTarget {
    type Error = ValidateNodeError;

    fn try_from(value: &'static [u8]) -> Result<Self, Self::Error> {
        if value.is_empty() || value.contains(&b'\0') {
            return Err(ValidateNodeError::InvalidSymlinkTarget(
                bytes::Bytes::from_static(value),
            ));
        }

        Ok(Self {
            inner: bytes::Bytes::from_static(value),
        })
    }
}

impl TryFrom<&str> for SymlinkTarget {
    type Error = ValidateNodeError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.is_empty() {
            return Err(ValidateNodeError::InvalidSymlinkTarget(
                bytes::Bytes::copy_from_slice(value.as_bytes()),
            ));
        }

        Ok(Self {
            inner: bytes::Bytes::copy_from_slice(value.as_bytes()),
        })
    }
}

impl Debug for SymlinkTarget {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(self.inner.as_bstr(), f)
    }
}

impl Display for SymlinkTarget {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(self.inner.as_bstr(), f)
    }
}
