use data_encoding::{BASE64, BASE64_NOPAD, HEXLOWER};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use thiserror::Error;

use crate::nixbase32;

pub use crate::nixhash_with_mode::NixHashWithMode;

/// Nix allows specifying hashes in various encodings, and magically just
/// derives the encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NixHash {
    pub digest: Vec<u8>,

    pub algo: HashAlgo,
}

impl NixHash {
    /// Constructs a new [NixHash] by specifying [HashAlgo] and digest.
    pub fn new(algo: HashAlgo, digest: Vec<u8>) -> Self {
        Self { algo, digest }
    }

    /// Formats a [NixHash] in the Nix default hash format,
    /// which is the algo, followed by a colon, then the lower hex encoded digest.
    pub fn to_nix_hash_string(&self) -> String {
        format!("{}:{}", self.algo, HEXLOWER.encode(&self.digest))
    }
}

/// This are the hash algorithms supported by cppnix.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HashAlgo {
    Md5,
    Sha1,
    Sha256,
    Sha512,
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

/// Errors related to NixHash construction.
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid hash algo: {0}")]
    InvalidAlgo(String),
    #[error("invalid SRI string: {0}")]
    InvalidSRI(String),
    #[error("invalid encoded digest length '{0}' for algo {1}")]
    InvalidEncodedDigestLength(usize, HashAlgo),
    #[error("invalid base16 encoding: {0}")]
    InvalidBase16Encoding(data_encoding::DecodeError),
    #[error("invalid base32 encoding: {0}")]
    InvalidBase32Encoding(nixbase32::Nixbase32DecodeError),
    #[error("invalid base64 encoding: {0}")]
    InvalidBase64Encoding(data_encoding::DecodeError),
    #[error("conflicting hash algo: {0} (hash_algo) vs {1} (inline)")]
    ConflictingHashAlgos(String, String),
    #[error("missing inline hash algo, but no externally-specified algo: {0}")]
    MissingInlineHashAlgo(String),
}

/// parses a string to a nix hash.
///
/// Hashes can be:
/// - Nix hash strings
/// - SRI hashes
/// - bare digests
///
/// Encoding for Nix hash strings or bare digests can be:
/// - base16 (lowerhex),
/// - nixbase32,
/// - base64 (StdEncoding)
/// - sri string
///
/// The encoding is derived from the length of the string and the hash type.
/// The hash is communicated out-of-band, but might also be in-band (in the
/// case of a nix hash string or SRI), in which it needs to be consistent with the
/// one communicated out-of-band.
pub fn from_str(s: &str, algo_str: Option<&str>) -> Result<NixHash, Error> {
    // validate algo_str, construct hash_algo
    let algo: Option<HashAlgo> = match &algo_str {
        Some("sha1") => Some(HashAlgo::Sha1),
        Some("sha256") => Some(HashAlgo::Sha256),
        Some("sha512") => Some(HashAlgo::Sha512),
        Some("md5") => Some(HashAlgo::Md5),
        Some(e) => return Err(Error::InvalidAlgo(e.to_string())),
        None => None,
    };

    // peek at the beginning of the string. Let's detect the SRI path first.
    if s.starts_with("sha1-")
        || s.starts_with("sha256-")
        || s.starts_with("sha512-")
        || s.starts_with("md5-")
    {
        let parsed_nixhash = from_sri_str(s)?;
        // ensure the algo matches with what has been passed externally, if so.
        if let Some(algo) = algo {
            if algo != parsed_nixhash.algo {
                return Err(Error::ConflictingHashAlgos(
                    algo.to_string(),
                    parsed_nixhash.algo.to_string(),
                ));
            }
        }
        return Ok(parsed_nixhash);
    }

    // Now, peek at the beginning again to see if it's a Nix Hash
    if s.starts_with("sha1:")
        || s.starts_with("sha256:")
        || s.starts_with("sha512:")
        || s.starts_with("md5:")
    {
        let parsed_nixhash = from_nix_str(s)?;
        // ensure the algo matches with what has been passed externally, if so.
        if let Some(algo) = algo {
            if algo != parsed_nixhash.algo {
                return Err(Error::ConflictingHashAlgos(
                    algo.to_string(),
                    parsed_nixhash.algo.to_string(),
                ));
            }
        }
        return Ok(parsed_nixhash);
    }

    // In all other cases, we assume a bare digest, so there MUST be an externally-passed algo.
    match algo {
        // Fail if there isn't.
        None => Err(Error::MissingInlineHashAlgo(s.to_string())),
        Some(algo) => decode_digest(s, algo),
    }
}

/// Parses a Nix hash string ($algo:$digest) to a NixHash.
pub fn from_nix_str(s: &str) -> Result<NixHash, Error> {
    if let Some(rest) = s.strip_prefix("sha1:") {
        decode_digest(rest, HashAlgo::Sha1)
    } else if let Some(rest) = s.strip_prefix("sha256:") {
        decode_digest(rest, HashAlgo::Sha256)
    } else if let Some(rest) = s.strip_prefix("sha512:") {
        decode_digest(rest, HashAlgo::Sha512)
    } else if let Some(rest) = s.strip_prefix("md5:") {
        decode_digest(rest, HashAlgo::Md5)
    } else {
        Err(Error::InvalidAlgo(s.to_string()))
    }
}

/// Parses a Nix SRI string to a NixHash.
/// Contrary to the SRI spec, Nix doesn't support SRI strings with multiple hashes,
/// only supports sha256 and sha512 from the spec, and supports sha1 and md5
/// additionally.
/// It also accepts SRI strings where the base64 has an with invalid padding.
pub fn from_sri_str(s: &str) -> Result<NixHash, Error> {
    // try to find the first occurence of "-"
    let idx = s.as_bytes().iter().position(|&e| e == b'-');

    if idx.is_none() {
        return Err(Error::InvalidSRI(s.to_string()));
    }

    let idx = idx.unwrap();

    // try to map the part before that `-` to a supported hash algo:
    let algo: HashAlgo = s[..idx].try_into()?;

    // the rest should be the digest (as Nix doesn't support more than one hash in an SRI string).
    let encoded_digest = &s[idx + 1..];
    let actual_len = encoded_digest.as_bytes().len();

    // verify the digest length matches what we'd expect from the hash function,
    // and then either try decoding as BASE64 or BASE64_NOPAD.
    // This will also reject SRI strings with more than one hash, because the length won't match
    if actual_len == BASE64.encode_len(hash_algo_length(&algo)) {
        let digest: Vec<u8> = BASE64
            .decode(encoded_digest.as_bytes())
            .map_err(Error::InvalidBase64Encoding)?;
        Ok(NixHash { digest, algo })
    } else if actual_len == BASE64_NOPAD.encode_len(hash_algo_length(&algo)) {
        let digest: Vec<u8> = BASE64_NOPAD
            .decode(encoded_digest.as_bytes())
            .map_err(Error::InvalidBase64Encoding)?;
        Ok(NixHash { digest, algo })
    } else {
        // NOTE: As of now, we reject SRI hashes containing additional
        // characters (which upstream Nix seems to simply truncate), as
        // there's no occurence of this is in nixpkgs.
        // It most likely should also be a bug in Nix.
        Err(Error::InvalidEncodedDigestLength(
            encoded_digest.as_bytes().len(),
            algo,
        ))
    }
}

/// decode a plain digest depending on the hash algo specified externally.
fn decode_digest(s: &str, algo: HashAlgo) -> Result<NixHash, Error> {
    // for the chosen hash algo, calculate the expected (decoded) digest length
    // (as bytes)
    let expected_digest_len = hash_algo_length(&algo);

    Ok(NixHash {
        digest: match s.len() {
            n if n == data_encoding::HEXLOWER.encode_len(expected_digest_len) => {
                data_encoding::HEXLOWER
                    .decode(s.as_ref())
                    .map_err(Error::InvalidBase16Encoding)
            }
            n if n == nixbase32::encode_len(expected_digest_len) => {
                nixbase32::decode(s.as_ref()).map_err(Error::InvalidBase32Encoding)
            }
            n if n == BASE64.encode_len(expected_digest_len) => BASE64
                .decode(s.as_ref())
                .map_err(Error::InvalidBase64Encoding),
            _ => return Err(Error::InvalidEncodedDigestLength(s.len(), algo)),
        }?,
        algo,
    })
}

// return the number of bytes in the digest of the given hash algo.
fn hash_algo_length(hash_algo: &HashAlgo) -> usize {
    match hash_algo {
        HashAlgo::Sha1 => 20,
        HashAlgo::Sha256 => 32,
        HashAlgo::Sha512 => 64,
        HashAlgo::Md5 => 16,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        nixbase32,
        nixhash::{self, HashAlgo, NixHash},
    };
    use test_case::test_case;
    const DIGEST_SHA1: &[u8] = &[
        0x60, 0x16, 0x77, 0x79, 0x97, 0xc3, 0x0a, 0xb0, 0x24, 0x13, 0xcf, 0x50, 0x95, 0x62, 0x2c,
        0xd7, 0x92, 0x42, 0x83, 0xac,
    ];

    const DIGEST_SHA256: &[u8] = &[
        0xa5, 0xce, 0x9c, 0x15, 0x5e, 0xd0, 0x93, 0x97, 0x61, 0x46, 0x46, 0xc9, 0x71, 0x7f, 0xc7,
        0xcd, 0x94, 0xb1, 0x02, 0x3d, 0x7b, 0x76, 0xb6, 0x18, 0xd4, 0x09, 0xe4, 0xfe, 0xfd, 0x6e,
        0x9d, 0x39,
    ];

    const DIGEST_SHA512: &[u8] = &[
        0xab, 0x40, 0xd0, 0xbe, 0x35, 0x41, 0xf0, 0x77, 0x4b, 0xba, 0x78, 0x15, 0xd1, 0x3d, 0x10,
        0xb0, 0x32, 0x52, 0xe9, 0x6e, 0x95, 0xf7, 0xdb, 0xb4, 0xee, 0x99, 0xa3, 0xb4, 0x31, 0xc2,
        0x16, 0x62, 0xfd, 0x69, 0x71, 0xa0, 0x20, 0x16, 0x0e, 0x39, 0x84, 0x8a, 0xa5, 0xf3, 0x05,
        0xb9, 0xbe, 0x0f, 0x78, 0x72, 0x7b, 0x2b, 0x07, 0x89, 0xe3, 0x9f, 0x12, 0x4d, 0x21, 0xe9,
        0x2b, 0x8f, 0x39, 0xef,
    ];
    const DIGEST_MD5: &[u8] = &[
        0xc4, 0x87, 0x4a, 0x88, 0x97, 0x44, 0x0b, 0x39, 0x3d, 0x86, 0x2d, 0x8f, 0xd4, 0x59, 0x07,
        0x3f,
    ];

    fn to_base16(digest: &[u8]) -> String {
        data_encoding::HEXLOWER.encode(digest)
    }

    fn to_nixbase32(digest: &[u8]) -> String {
        nixbase32::encode(digest)
    }

    fn to_base64(digest: &[u8]) -> String {
        data_encoding::BASE64.encode(digest)
    }

    fn to_base64_nopad(digest: &[u8]) -> String {
        data_encoding::BASE64_NOPAD.encode(digest)
    }

    // TODO
    fn make_nixhash(algo: &HashAlgo, digest_encoded: String) -> String {
        format!("{}:{}", algo, digest_encoded)
    }
    fn make_sri_string(algo: &HashAlgo, digest_encoded: String) -> String {
        format!("{}-{}", algo, digest_encoded)
    }

    /// Test parsing a hash string in various formats, and also when/how the out-of-band algo is needed.
    #[test_case(DIGEST_SHA1, HashAlgo::Sha1; "sha1")]
    #[test_case(DIGEST_SHA256, HashAlgo::Sha256; "sha256")]
    #[test_case(DIGEST_SHA512, HashAlgo::Sha512; "sha512")]
    #[test_case(DIGEST_MD5, HashAlgo::Md5; "md5")]
    fn from_str(digest: &[u8], algo: HashAlgo) {
        let expected_hash = NixHash {
            digest: digest.to_vec(),
            algo: algo.clone(),
        };
        // parse SRI
        {
            // base64 without out-of-band algo
            let s = make_sri_string(&algo, to_base64(digest));
            let h = nixhash::from_str(&s, None).expect("must succeed");
            assert_eq!(expected_hash, h);

            // base64 with out-of-band-algo
            let s = make_sri_string(&algo, to_base64(digest));
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, h);

            // base64_nopad without out-of-band algo
            let s = make_sri_string(&algo, to_base64_nopad(digest));
            let h = nixhash::from_str(&s, None).expect("must succeed");
            assert_eq!(expected_hash, h);

            // base64_nopad with out-of-band-algo
            let s = make_sri_string(&algo, to_base64_nopad(digest));
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, h);
        }

        // parse plain base16. should succeed with algo out-of-band, but fail without.
        {
            let s = to_base16(digest);
            nixhash::from_str(&s, None).expect_err("must fail");
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, h);
        }

        // parse plain nixbase32. should succeed with algo out-of-band, but fail without.
        {
            let s = to_nixbase32(digest);
            nixhash::from_str(&s, None).expect_err("must fail");
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, h);
        }

        // parse plain base64. should succeed with algo out-of-band, but fail without.
        {
            let s = to_base64(digest);
            nixhash::from_str(&s, None).expect_err("must fail");
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, h);
        }

        // parse Nix hash strings
        {
            // base16. should succeed with both algo out-of-band and in-band.
            {
                let s = make_nixhash(&algo, to_base16(digest));
                assert_eq!(
                    expected_hash,
                    nixhash::from_str(&s, None).expect("must succeed")
                );
                assert_eq!(
                    expected_hash,
                    nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed")
                );
            }
            // nixbase32. should succeed with both algo out-of-band and in-band.
            {
                let s = make_nixhash(&algo, to_nixbase32(digest));
                assert_eq!(
                    expected_hash,
                    nixhash::from_str(&s, None).expect("must succeed")
                );
                assert_eq!(
                    expected_hash,
                    nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed")
                );
            }
            // base64. should succeed with both algo out-of-band and in-band.
            {
                let s = make_nixhash(&algo, to_base64(digest));
                assert_eq!(
                    expected_hash,
                    nixhash::from_str(&s, None).expect("must succeed")
                );
                assert_eq!(
                    expected_hash,
                    nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed")
                );
            }
        }
    }

    /// Test parsing an SRI hash via the [nixhash::from_sri_str] method.
    #[test]
    fn from_sri_str() {
        let nix_hash = nixhash::from_sri_str("sha256-pc6cFV7Qk5dhRkbJcX/HzZSxAj17drYY1Ank/v1unTk=")
            .expect("must succeed");

        assert_eq!(HashAlgo::Sha256, nix_hash.algo);
        assert_eq!(
            vec![
                0xa5, 0xce, 0x9c, 0x15, 0x5e, 0xd0, 0x93, 0x97, 0x61, 0x46, 0x46, 0xc9, 0x71, 0x7f,
                0xc7, 0xcd, 0x94, 0xb1, 0x02, 0x3d, 0x7b, 0x76, 0xb6, 0x18, 0xd4, 0x09, 0xe4, 0xfe,
                0xfd, 0x6e, 0x9d, 0x39
            ],
            nix_hash.digest
        )
    }

    /// Ensure we detect truncated base64 digests, where the digest size
    /// doesn't match what's expected from that hash function.
    #[test]
    fn from_sri_str_truncated() {
        nixhash::from_sri_str("sha256-pc6cFV7Qk5dhRkbJcX/HzZSxAj17drYY1Ank")
            .expect_err("must fail");
    }

    /// Ensure we fail on SRI hashes that Nix doesn't support.
    #[test]
    fn from_sri_str_unsupported() {
        nixhash::from_sri_str(
            "sha384-o4UVSl89mIB0sFUK+3jQbG+C9Zc9dRlV/Xd3KAvXEbhqxu0J5OAdg6b6VHKHwQ7U",
        )
        .expect_err("must fail");
    }

    /// Ensure we reject invalid base64 encoding
    #[test]
    fn from_sri_str_invalid_base64() {
        nixhash::from_sri_str("sha256-invalid=base64").expect_err("must fail");
    }

    /// Ensure we reject SRI strings with multiple hashes, as Nix doesn't support that.
    #[test]
    fn from_sri_str_unsupported_multiple() {
        nixhash::from_sri_str("sha256-ngth6szLtC1IJIYyz3lhftzL8SkrJkqPyPve+dGqa1Y= sha512-q0DQvjVB8HdLungV0T0QsDJS6W6V99u07pmjtDHCFmL9aXGgIBYOOYSKpfMFub4PeHJ7KweJ458STSHpK4857w==").expect_err("must fail");
    }

    /// Nix also accepts SRI strings with missing padding, but only in case the
    /// string is expressed as SRI, so it still needs to have a `sha256-` prefix.
    ///
    /// This both seems to work if it is passed with and without specifying the
    /// hash algo out-of-band (hash = "sha256-…" or sha256 = "sha256-…")
    ///
    /// Passing the same broken base64 string, but not as SRI, while passing
    /// the hash algo out-of-band does not work.
    #[test]
    fn sha256_broken_padding() {
        let broken_base64 = "fgIr3TyFGDAXP5+qoAaiMKDg/a1MlT6Fv/S/DaA24S8";
        // if padded with a trailing '='
        let expected_digest = vec![
            0x7e, 0x02, 0x2b, 0xdd, 0x3c, 0x85, 0x18, 0x30, 0x17, 0x3f, 0x9f, 0xaa, 0xa0, 0x06,
            0xa2, 0x30, 0xa0, 0xe0, 0xfd, 0xad, 0x4c, 0x95, 0x3e, 0x85, 0xbf, 0xf4, 0xbf, 0x0d,
            0xa0, 0x36, 0xe1, 0x2f,
        ];

        // passing hash algo out of band should succeed
        let nix_hash = nixhash::from_str(&format!("sha256-{}", &broken_base64), Some("sha256"))
            .expect("must succeed");
        assert_eq!(&expected_digest, &nix_hash.digest);

        // not passing hash algo out of band should succeed
        let nix_hash =
            nixhash::from_str(&format!("sha256-{}", &broken_base64), None).expect("must succeed");
        assert_eq!(&expected_digest, &nix_hash.digest);

        // not passing SRI, but hash algo out of band should fail
        nixhash::from_str(broken_base64, Some("sha256")).expect_err("must fail");
    }
}
