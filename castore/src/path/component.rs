// TODO: split out this error
use crate::DirectoryError;

use bstr::ByteSlice;
use std::fmt::{self, Debug, Display};

/// A wrapper type for validated path components in the castore model.
/// Internally uses a [bytes::Bytes], but disallows
/// slashes, and null bytes to be present, as well as
/// '.', '..' and the empty string.
#[repr(transparent)]
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathComponent {
    pub(super) inner: bytes::Bytes,
}

impl AsRef<[u8]> for PathComponent {
    fn as_ref(&self) -> &[u8] {
        self.inner.as_ref()
    }
}

impl From<PathComponent> for bytes::Bytes {
    fn from(value: PathComponent) -> Self {
        value.inner
    }
}

pub(super) fn is_valid_name<B: AsRef<[u8]>>(name: B) -> bool {
    let v = name.as_ref();

    !v.is_empty() && v != *b".." && v != *b"." && !v.contains(&0x00) && !v.contains(&b'/')
}

impl TryFrom<bytes::Bytes> for PathComponent {
    type Error = DirectoryError;

    fn try_from(value: bytes::Bytes) -> Result<Self, Self::Error> {
        if !is_valid_name(&value) {
            return Err(DirectoryError::InvalidName(value));
        }

        Ok(Self { inner: value })
    }
}

impl TryFrom<&'static [u8]> for PathComponent {
    type Error = DirectoryError;

    fn try_from(value: &'static [u8]) -> Result<Self, Self::Error> {
        if !is_valid_name(value) {
            return Err(DirectoryError::InvalidName(bytes::Bytes::from_static(
                value,
            )));
        }
        Ok(Self {
            inner: bytes::Bytes::from_static(value),
        })
    }
}

impl TryFrom<&str> for PathComponent {
    type Error = DirectoryError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if !is_valid_name(value) {
            return Err(DirectoryError::InvalidName(bytes::Bytes::copy_from_slice(
                value.as_bytes(),
            )));
        }
        Ok(Self {
            inner: bytes::Bytes::copy_from_slice(value.as_bytes()),
        })
    }
}

impl TryFrom<&std::ffi::CStr> for PathComponent {
    type Error = DirectoryError;

    fn try_from(value: &std::ffi::CStr) -> Result<Self, Self::Error> {
        if !is_valid_name(value.to_bytes()) {
            return Err(DirectoryError::InvalidName(bytes::Bytes::copy_from_slice(
                value.to_bytes(),
            )));
        }
        Ok(Self {
            inner: bytes::Bytes::copy_from_slice(value.to_bytes()),
        })
    }
}

impl Debug for PathComponent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Debug::fmt(self.inner.as_bstr(), f)
    }
}

impl Display for PathComponent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(self.inner.as_bstr(), f)
    }
}
