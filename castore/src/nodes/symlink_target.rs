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

/// The maximum length a symlink target can have.
/// Linux allows 4095 bytes here.
pub const MAX_TARGET_LEN: usize = 4095;

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

fn validate_symlink_target<B: AsRef<[u8]>>(symlink_target: B) -> Result<B, SymlinkTargetError> {
    let v = symlink_target.as_ref();

    if v.is_empty() {
        return Err(SymlinkTargetError::Empty);
    }
    if v.len() > MAX_TARGET_LEN {
        return Err(SymlinkTargetError::TooLong);
    }
    if v.contains(&0x00) {
        return Err(SymlinkTargetError::Null);
    }

    Ok(symlink_target)
}

impl TryFrom<bytes::Bytes> for SymlinkTarget {
    type Error = SymlinkTargetError;

    fn try_from(value: bytes::Bytes) -> Result<Self, Self::Error> {
        if let Err(e) = validate_symlink_target(&value) {
            return Err(SymlinkTargetError::Convert(value, Box::new(e)));
        }

        Ok(Self { inner: value })
    }
}

impl TryFrom<&'static [u8]> for SymlinkTarget {
    type Error = SymlinkTargetError;

    fn try_from(value: &'static [u8]) -> Result<Self, Self::Error> {
        if let Err(e) = validate_symlink_target(&value) {
            return Err(SymlinkTargetError::Convert(value.into(), Box::new(e)));
        }

        Ok(Self {
            inner: bytes::Bytes::from_static(value),
        })
    }
}

impl TryFrom<&str> for SymlinkTarget {
    type Error = SymlinkTargetError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Err(e) = validate_symlink_target(value) {
            return Err(SymlinkTargetError::Convert(
                value.to_owned().into(),
                Box::new(e),
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

/// Errors created when constructing / converting to [SymlinkTarget].
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(test, derive(Clone))]
pub enum SymlinkTargetError {
    #[error("cannot be empty")]
    Empty,
    #[error("cannot contain null bytes")]
    Null,
    #[error("cannot be over {} bytes long", MAX_TARGET_LEN)]
    TooLong,
    #[error("unable to convert '{:?}", .0.as_bstr())]
    Convert(bytes::Bytes, Box<Self>),
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;

    use super::validate_symlink_target;
    use super::{SymlinkTarget, SymlinkTargetError};

    #[rstest]
    #[case::empty(b"", SymlinkTargetError::Empty)]
    #[case::null(b"foo\0", SymlinkTargetError::Null)]
    fn errors(#[case] v: &'static [u8], #[case] err: SymlinkTargetError) {
        {
            assert_eq!(
                Err(err.clone()),
                validate_symlink_target(v),
                "validate_symlink_target must fail as expected"
            );
        }

        let exp_err_v = Bytes::from_static(v);

        // Bytes
        {
            let v = Bytes::from_static(v);
            assert_eq!(
                Err(SymlinkTargetError::Convert(
                    exp_err_v.clone(),
                    Box::new(err.clone())
                )),
                SymlinkTarget::try_from(v),
                "conversion must fail as expected"
            );
        }
        // &[u8]
        {
            assert_eq!(
                Err(SymlinkTargetError::Convert(
                    exp_err_v.clone(),
                    Box::new(err.clone())
                )),
                SymlinkTarget::try_from(v),
                "conversion must fail as expected"
            );
        }
        // &str, if this is valid UTF-8
        {
            if let Ok(v) = std::str::from_utf8(v) {
                assert_eq!(
                    Err(SymlinkTargetError::Convert(
                        exp_err_v.clone(),
                        Box::new(err.clone())
                    )),
                    SymlinkTarget::try_from(v),
                    "conversion must fail as expected"
                );
            }
        }
    }

    #[test]
    fn error_toolong() {
        assert_eq!(
            Err(SymlinkTargetError::TooLong),
            validate_symlink_target("X".repeat(5000).into_bytes().as_slice())
        )
    }

    #[rstest]
    #[case::boring(b"aa")]
    #[case::dot(b".")]
    #[case::dotsandslashes(b"./..")]
    #[case::dotdot(b"..")]
    #[case::slashes(b"a/b")]
    #[case::slashes_and_absolute(b"/a/b")]
    #[case::invalid_utf8(b"\xc5\xc4\xd6")]
    fn success(#[case] v: &'static [u8]) {
        let exp = SymlinkTarget { inner: v.into() };

        // Bytes
        {
            let v: Bytes = v.into();
            assert_eq!(
                Ok(exp.clone()),
                SymlinkTarget::try_from(v),
                "conversion must succeed"
            )
        }

        // &[u8]
        {
            assert_eq!(
                Ok(exp.clone()),
                SymlinkTarget::try_from(v),
                "conversion must succeed"
            )
        }

        // &str, if this is valid UTF-8
        {
            if let Ok(v) = std::str::from_utf8(v) {
                assert_eq!(
                    Ok(exp.clone()),
                    SymlinkTarget::try_from(v),
                    "conversion must succeed"
                )
            }
        }
    }
}
