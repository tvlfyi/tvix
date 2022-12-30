use crate::nixbase32::NIXBASE32;
use data_encoding::DecodeError;
use std::fmt;
use thiserror::Error;

const PATH_HASH_SIZE: usize = 20;

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
    digest: [u8; PATH_HASH_SIZE],
    name: String,
}

impl NixPath {
    pub fn from_string(s: &str) -> Result<NixPath, ParseNixPathError> {
        let encoded_path_hash_size = NIXBASE32.encode_len(PATH_HASH_SIZE);
        let name_offset = encoded_path_hash_size + 1;

        // the whole string needs to be at least:
        //
        // - 32 characters (encoded hash)
        // - 1 dash
        // - 1 character for the name
        if s.len() < name_offset + 1 {
            return Err(ParseNixPathError::InvalidName("".to_string()));
        }

        let digest = match NIXBASE32.decode(s[..encoded_path_hash_size].as_bytes()) {
            Ok(decoded) => decoded,
            Err(decoder_error) => {
                return Err(ParseNixPathError::InvalidHashEncoding(decoder_error))
            }
        };

        if s.as_bytes()[encoded_path_hash_size] != b'-' {
            return Err(ParseNixPathError::MissingDash());
        }

        NixPath::validate_characters(&s[name_offset..])?;

        // copy the digest:Vec<u8> to a [u8; PATH_HASH_SIZE]
        let mut buffer: [u8; PATH_HASH_SIZE] = [0; PATH_HASH_SIZE];
        buffer[..PATH_HASH_SIZE].copy_from_slice(&digest[..PATH_HASH_SIZE]);

        Ok(NixPath {
            name: s[name_offset..].to_string(),
            digest: buffer,
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
    use crate::nixpath::PATH_HASH_SIZE;

    use super::NixPath;

    #[test]
    fn happy_path() {
        let example_nix_path_str =
            "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p20170221182432";
        let nixpath =
            NixPath::from_string(&example_nix_path_str).expect("Error parsing example string");

        let expected_digest: [u8; PATH_HASH_SIZE] = [
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
