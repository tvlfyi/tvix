use crate::nixbase32::NIXBASE32;
use data_encoding::DecodeError;
use std::fmt;
use thiserror::Error;

pub const DIGEST_SIZE: usize = 20;
// lazy_static doesn't allow us to call NIXBASE32.encode_len(), so we ran it
// manually and have an assert in the tests.
pub const ENCODED_DIGEST_SIZE: usize = 32;

// The store dir prefix, without trailing slash.
// That's usually where the Nix store is mounted at.
pub const STORE_DIR: &str = "/nix/store";
pub const STORE_DIR_WITH_SLASH: &str = "/nix/store/";

/// Errors that can occur during the validation of name characters.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum ParseStorePathError {
    #[error("Dash is missing between hash and name")]
    MissingDash(),
    #[error("Hash encoding is invalid: {0}")]
    InvalidHashEncoding(DecodeError),
    #[error("Invalid name: {0}")]
    InvalidName(String),
    #[error("Tried to parse an absolute path which was missing the store dir prefix.")]
    MissingStoreDir(),
}

/// Represents a path in the Nix store (a direct child of [STORE_DIR]).
///
/// It starts with a digest (20 bytes), [struct@NIXBASE32]-encoded, followed by
/// a `-`, and ends with a `name`, which is a string, consisting only of ASCCI
/// alphanumeric characters, or one of the following characters: `-`, `_`, `.`,
/// `+`, `?`, `=`.
///
/// The name is usually used to describe the pname and version of a package.
/// Derivations paths can also be represented as store paths, they end
/// with .drv.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StorePath {
    pub digest: [u8; DIGEST_SIZE],
    pub name: String,
}

impl StorePath {
    pub fn from_string(s: &str) -> Result<StorePath, ParseStorePathError> {
        // the whole string needs to be at least:
        //
        // - 32 characters (encoded hash)
        // - 1 dash
        // - 1 character for the name
        if s.len() < ENCODED_DIGEST_SIZE + 2 {
            return Err(ParseStorePathError::InvalidName("".to_string()));
        }

        let digest = match NIXBASE32.decode(s[..ENCODED_DIGEST_SIZE].as_bytes()) {
            Ok(decoded) => decoded,
            Err(decoder_error) => {
                return Err(ParseStorePathError::InvalidHashEncoding(decoder_error))
            }
        };

        if s.as_bytes()[ENCODED_DIGEST_SIZE] != b'-' {
            return Err(ParseStorePathError::MissingDash());
        }

        StorePath::validate_name(&s[ENCODED_DIGEST_SIZE + 2..])?;

        Ok(StorePath {
            name: s[ENCODED_DIGEST_SIZE + 1..].to_string(),
            digest: digest.try_into().expect("size is known"),
        })
    }

    /// Construct a [StorePath] from an absolute store path string.
    /// That is a string starting with the store prefix (/nix/store)
    pub fn from_absolute_path(s: &str) -> Result<StorePath, ParseStorePathError> {
        match s.strip_prefix(STORE_DIR_WITH_SLASH) {
            Some(s_stripped) => Self::from_string(s_stripped),
            None => Err(ParseStorePathError::MissingStoreDir()),
        }
    }

    // Converts the [StorePath] to an absolute store path string.
    /// That is a string starting with the store prefix (/nix/store)
    pub fn to_absolute_path(&self) -> String {
        format!("{}/{}", STORE_DIR, self)
    }

    /// Checks a given &str to match the restrictions for store path names.
    pub fn validate_name(s: &str) -> Result<(), ParseStorePathError> {
        for c in s.chars() {
            if c.is_ascii_alphanumeric()
                || c == '-'
                || c == '_'
                || c == '.'
                || c == '+'
                || c == '?'
                || c == '='
            {
                continue;
            }

            return Err(ParseStorePathError::InvalidName(s.to_string()));
        }

        Ok(())
    }
}

impl fmt::Display for StorePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}-{}",
            crate::nixbase32::NIXBASE32.encode(&self.digest),
            self.name
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::nixbase32::NIXBASE32;
    use crate::store_path::{DIGEST_SIZE, ENCODED_DIGEST_SIZE};

    use super::{ParseStorePathError, StorePath};

    #[test]
    fn encoded_digest_size() {
        assert_eq!(ENCODED_DIGEST_SIZE, NIXBASE32.encode_len(DIGEST_SIZE));
    }

    #[test]
    fn happy_path() {
        let example_nix_path_str =
            "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432";
        let nixpath =
            StorePath::from_string(&example_nix_path_str).expect("Error parsing example string");

        let expected_digest: [u8; DIGEST_SIZE] = [
            0x8a, 0x12, 0x32, 0x15, 0x22, 0xfd, 0x91, 0xef, 0xbd, 0x60, 0xeb, 0xb2, 0x48, 0x1a,
            0xf8, 0x85, 0x80, 0xf6, 0x16, 0x00,
        ];

        assert_eq!("net-tools-1.60_p20170221182432", nixpath.name);
        assert_eq!(nixpath.digest, expected_digest);

        assert_eq!(example_nix_path_str, nixpath.to_string())
    }

    #[test]
    fn invalid_hash_length() {
        StorePath::from_string("00bgd045z0d4icpbc2yy-net-tools-1.60_p20170221182432")
            .expect_err("No error raised.");
    }

    #[test]
    fn invalid_encoding_hash() {
        StorePath::from_string("00bgd045z0d4icpbc2yyz4gx48aku4la-net-tools-1.60_p20170221182432")
            .expect_err("No error raised.");
    }

    #[test]
    fn more_than_just_the_bare_nix_store_path() {
        StorePath::from_string(
            "00bgd045z0d4icpbc2yyz4gx48aku4la-net-tools-1.60_p20170221182432/bin/arp",
        )
        .expect_err("No error raised.");
    }

    #[test]
    fn no_dash_between_hash_and_name() {
        StorePath::from_string("00bgd045z0d4icpbc2yyz4gx48ak44lanet-tools-1.60_p20170221182432")
            .expect_err("No error raised.");
    }

    #[test]
    fn absolute_path() {
        let example_nix_path_str =
            "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432";
        let nixpath_expected = StorePath::from_string(&example_nix_path_str).expect("must parse");

        let nixpath_actual = StorePath::from_absolute_path(
            "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432",
        )
        .expect("must parse");

        assert_eq!(nixpath_expected, nixpath_actual);

        assert_eq!(
            "/nix/store/00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432",
            nixpath_actual.to_absolute_path(),
        );
    }

    #[test]
    fn absolute_path_missing_prefix() {
        assert_eq!(
            ParseStorePathError::MissingStoreDir(),
            StorePath::from_absolute_path("foobar-123").expect_err("must fail")
        );
    }
}
