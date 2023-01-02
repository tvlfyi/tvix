use crate::nixbase32::NIXBASE32;
use data_encoding::DecodeError;
use std::fmt;
use thiserror::Error;

pub const DIGEST_SIZE: usize = 20;
// lazy_static doesn't allow us to call NIXBASE32.encode_len(), so we ran it
// manually and have an assert in the tests.
pub const ENCODED_DIGEST_SIZE: usize = 32;

/// Errors that can occur during the validation of name characters.
#[derive(Debug, PartialEq, Eq, Error)]
pub enum ParseNixPathError {
    #[error("Dash is missing")]
    MissingDash(),
    #[error("Hash encoding is invalid {0}")]
    InvalidHashEncoding(DecodeError),
    #[error("Invalid name {0}")]
    InvalidName(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct NixPath {
    pub digest: [u8; DIGEST_SIZE],
    pub name: String,
}

impl NixPath {
    pub fn from_string(s: &str) -> Result<NixPath, ParseNixPathError> {
        // the whole string needs to be at least:
        //
        // - 32 characters (encoded hash)
        // - 1 dash
        // - 1 character for the name
        if s.len() < ENCODED_DIGEST_SIZE + 2 {
            return Err(ParseNixPathError::InvalidName("".to_string()));
        }

        let digest = match NIXBASE32.decode(s[..ENCODED_DIGEST_SIZE].as_bytes()) {
            Ok(decoded) => decoded,
            Err(decoder_error) => {
                return Err(ParseNixPathError::InvalidHashEncoding(decoder_error))
            }
        };

        if s.as_bytes()[ENCODED_DIGEST_SIZE] != b'-' {
            return Err(ParseNixPathError::MissingDash());
        }

        NixPath::validate_characters(&s[ENCODED_DIGEST_SIZE + 2..])?;

        Ok(NixPath {
            name: s[ENCODED_DIGEST_SIZE + 1..].to_string(),
            digest: digest.try_into().expect("size is known"),
        })
    }

    fn validate_characters(s: &str) -> Result<(), ParseNixPathError> {
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

            return Err(ParseNixPathError::InvalidName(s.to_string()));
        }

        Ok(())
    }
}

impl fmt::Display for NixPath {
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
    use crate::nixpath::{DIGEST_SIZE, ENCODED_DIGEST_SIZE};

    use super::NixPath;

    #[test]
    fn encoded_digest_size() {
        assert_eq!(ENCODED_DIGEST_SIZE, NIXBASE32.encode_len(DIGEST_SIZE));
    }

    #[test]
    fn happy_path() {
        let example_nix_path_str =
            "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432";
        let nixpath =
            NixPath::from_string(&example_nix_path_str).expect("Error parsing example string");

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
        NixPath::from_string("00bgd045z0d4icpbc2yy-net-tools-1.60_p20170221182432")
            .expect_err("No error raised.");
    }

    #[test]
    fn invalid_encoding_hash() {
        NixPath::from_string("00bgd045z0d4icpbc2yyz4gx48aku4la-net-tools-1.60_p20170221182432")
            .expect_err("No error raised.");
    }

    #[test]
    fn more_than_just_the_bare_nix_store_path() {
        NixPath::from_string(
            "00bgd045z0d4icpbc2yyz4gx48aku4la-net-tools-1.60_p20170221182432/bin/arp",
        )
        .expect_err("No error raised.");
    }

    #[test]
    fn no_dash_between_hash_and_name() {
        NixPath::from_string("00bgd045z0d4icpbc2yyz4gx48ak44lanet-tools-1.60_p20170221182432")
            .expect_err("No error raised.");
    }
}
