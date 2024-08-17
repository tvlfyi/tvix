use bstr::ByteSlice;
use std::fmt::{self, Debug, Display};

/// A wrapper type for validated path components in the castore model.
/// Internally uses a [bytes::Bytes], but disallows
/// slashes, and null bytes to be present, as well as
/// '.', '..' and the empty string.
/// It also rejects components that are too long (> 255 bytes).
#[repr(transparent)]
#[derive(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct PathComponent {
    pub(super) inner: bytes::Bytes,
}

/// The maximum length an individual path component can have.
/// Linux allows 255 bytes of actual name, so we pick that.
pub const MAX_NAME_LEN: usize = 255;

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

pub(super) fn validate_name<B: AsRef<[u8]>>(name: B) -> Result<(), PathComponentError> {
    match name.as_ref() {
        b"" => Err(PathComponentError::Empty),
        b".." => Err(PathComponentError::Parent),
        b"." => Err(PathComponentError::CurDir),
        v if v.len() > MAX_NAME_LEN => Err(PathComponentError::TooLong),
        v if v.contains(&0x00) => Err(PathComponentError::Null),
        v if v.contains(&b'/') => Err(PathComponentError::Slashes),
        _ => Ok(()),
    }
}

impl TryFrom<bytes::Bytes> for PathComponent {
    type Error = PathComponentError;

    fn try_from(value: bytes::Bytes) -> Result<Self, Self::Error> {
        if let Err(e) = validate_name(&value) {
            return Err(PathComponentError::Convert(value, Box::new(e)));
        }

        Ok(Self { inner: value })
    }
}

impl TryFrom<&'static [u8]> for PathComponent {
    type Error = PathComponentError;

    fn try_from(value: &'static [u8]) -> Result<Self, Self::Error> {
        if let Err(e) = validate_name(value) {
            return Err(PathComponentError::Convert(value.into(), Box::new(e)));
        }

        Ok(Self {
            inner: bytes::Bytes::from_static(value),
        })
    }
}

impl TryFrom<&str> for PathComponent {
    type Error = PathComponentError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Err(e) = validate_name(value) {
            return Err(PathComponentError::Convert(
                value.to_owned().into(),
                Box::new(e),
            ));
        }

        Ok(Self {
            inner: bytes::Bytes::copy_from_slice(value.as_bytes()),
        })
    }
}

impl TryFrom<&std::ffi::CStr> for PathComponent {
    type Error = PathComponentError;

    fn try_from(value: &std::ffi::CStr) -> Result<Self, Self::Error> {
        let value = value.to_bytes();
        if let Err(e) = validate_name(value) {
            return Err(PathComponentError::Convert(
                value.to_owned().into(),
                Box::new(e),
            ));
        }

        Ok(Self {
            inner: bytes::Bytes::copy_from_slice(value),
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

/// Errors created when parsing / validating [PathComponent].
#[derive(Debug, PartialEq, thiserror::Error)]
#[cfg_attr(test, derive(Clone))]
pub enum PathComponentError {
    #[error("cannot be empty")]
    Empty,
    #[error("cannot contain null bytes")]
    Null,
    #[error("cannot be '.'")]
    CurDir,
    #[error("cannot be '..'")]
    Parent,
    #[error("cannot contain slashes")]
    Slashes,
    #[error("cannot be over {} bytes long", MAX_NAME_LEN)]
    TooLong,
    #[error("unable to convert '{:?}'", .0.as_bstr())]
    Convert(bytes::Bytes, #[source] Box<Self>),
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;

    use bytes::Bytes;
    use rstest::rstest;

    use super::{validate_name, PathComponent, PathComponentError};

    #[rstest]
    #[case::empty(b"", PathComponentError::Empty)]
    #[case::null(b"foo\0", PathComponentError::Null)]
    #[case::curdir(b".", PathComponentError::CurDir)]
    #[case::parent(b"..", PathComponentError::Parent)]
    #[case::slashes1(b"a/b", PathComponentError::Slashes)]
    #[case::slashes2(b"/", PathComponentError::Slashes)]
    fn errors(#[case] v: &'static [u8], #[case] err: PathComponentError) {
        {
            assert_eq!(
                Err(err.clone()),
                validate_name(v),
                "validate_name must fail as expected"
            );
        }

        let exp_err_v = Bytes::from_static(v);

        // Bytes
        {
            let v = Bytes::from_static(v);
            assert_eq!(
                Err(PathComponentError::Convert(
                    exp_err_v.clone(),
                    Box::new(err.clone())
                )),
                PathComponent::try_from(v),
                "conversion must fail as expected"
            );
        }
        // &[u8]
        {
            assert_eq!(
                Err(PathComponentError::Convert(
                    exp_err_v.clone(),
                    Box::new(err.clone())
                )),
                PathComponent::try_from(v),
                "conversion must fail as expected"
            );
        }
        // &str, if it is valid UTF-8
        {
            if let Ok(v) = std::str::from_utf8(v) {
                assert_eq!(
                    Err(PathComponentError::Convert(
                        exp_err_v.clone(),
                        Box::new(err.clone())
                    )),
                    PathComponent::try_from(v),
                    "conversion must fail as expected"
                );
            }
        }
        // &CStr, if it can be constructed (fails if the payload contains null bytes)
        {
            if let Ok(v) = CString::new(v) {
                let v = v.as_ref();
                assert_eq!(
                    Err(PathComponentError::Convert(
                        exp_err_v.clone(),
                        Box::new(err.clone())
                    )),
                    PathComponent::try_from(v),
                    "conversion must fail as expected"
                );
            }
        }
    }

    #[test]
    fn error_toolong() {
        assert_eq!(
            Err(PathComponentError::TooLong),
            validate_name("X".repeat(500).into_bytes().as_slice())
        )
    }

    #[test]
    fn success() {
        let exp = PathComponent { inner: "aa".into() };

        // Bytes
        {
            let v: Bytes = "aa".into();
            assert_eq!(
                Ok(exp.clone()),
                PathComponent::try_from(v),
                "conversion must succeed"
            );
        }

        // &[u8]
        {
            let v: &[u8] = b"aa";
            assert_eq!(
                Ok(exp.clone()),
                PathComponent::try_from(v),
                "conversion must succeed"
            );
        }

        // &str
        {
            let v: &str = "aa";
            assert_eq!(
                Ok(exp.clone()),
                PathComponent::try_from(v),
                "conversion must succeed"
            );
        }

        // &CStr
        {
            let v = CString::new("aa").expect("CString must construct");
            let v = v.as_c_str();
            assert_eq!(
                Ok(exp.clone()),
                PathComponent::try_from(v),
                "conversion must succeed"
            );
        }
    }
}
