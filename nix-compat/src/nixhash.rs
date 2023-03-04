use data_encoding::{BASE64, BASE64_NOPAD};
use std::fmt::Display;
use thiserror::Error;

use crate::nixbase32;

/// Nix allows specifying hashes in various encodings, and magically just
/// derives the encoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NixHash {
    pub digest: Vec<u8>,
    pub algo: HashAlgo,
}

/// This are the hash algorithms supported by cppnix.
#[derive(Clone, Debug, Eq, PartialEq)]
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

// return the number of bytes in the digest of the given hash algo.
fn hash_algo_length(hash_algo: &HashAlgo) -> usize {
    match hash_algo {
        HashAlgo::Sha1 => 20,
        HashAlgo::Sha256 => 32,
        HashAlgo::Sha512 => 64,
        HashAlgo::Md5 => 16,
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
    #[error("conflicting hash algo: {0} (hash_algo) vs {1} (SRI)")]
    ConflictingHashAlgos(String, String),
}

/// decode a string depending on the hash algo specified externally.
fn decode_digest(s: &str, algo: HashAlgo) -> Result<NixHash, Error> {
    // for the chosen hash algo, calculate the expected digest length (as bytes)
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
            _ => {
                // another length than what we expected from the passed hash algo
                // try to parse as SRI
                let nix_hash = from_sri_str(s)?;

                // ensure the algo matches what was specified
                if algo != nix_hash.algo {
                    return Err(Error::ConflictingHashAlgos(
                        algo.to_string(),
                        nix_hash.algo.to_string(),
                    ));
                }

                // return
                return Ok(nix_hash);
            }
        }?,
        algo,
    })
}

/// parses a string to a nix hash.
///
/// strings can be encoded as:
/// - base16 (lowerhex),
/// - nixbase32,
/// - base64 (StdEncoding)
/// - sri string
///
/// The encoding is derived from the length of the string and the hash type.
/// The hash type may be omitted if the hash is expressed in SRI.
/// Even though SRI allows specifying multiple algorithms, Nix does only
/// support a single one.
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

    // in case the hash algo is set, decode the digest and return
    if let Some(algo) = algo {
        Ok(decode_digest(s, algo))?
    } else {
        // try to decode as SRI
        let nix_hash = from_sri_str(s)?;
        // and return
        Ok(nix_hash)
    }
}

/// Like [from_str], but only for SRI string.
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

#[cfg(test)]
mod tests {
    use crate::nixhash::{self, HashAlgo};

    const SHA256_SRI: &str = "sha256-pc6cFV7Qk5dhRkbJcX/HzZSxAj17drYY1Ank/v1unTk=";
    const SHA256_BASE16: &str = "a5ce9c155ed09397614646c9717fc7cd94b1023d7b76b618d409e4fefd6e9d39";
    const SHA256_NIXBASE32: &str = "0fcxdvyzxr09shcbcxkv7l1b356dqxzp3ja68rhrg4yhbqarrkm5";
    const SHA256_BASE64: &str = "pc6cFV7Qk5dhRkbJcX/HzZSxAj17drYY1Ank/v1unTk=";

    const SHA1_SRI: &str = "sha1-YBZ3eZfDCrAkE89QlWIs15JCg6w=";
    const SHA1_BASE16: &str = "6016777997c30ab02413cf5095622cd7924283ac";
    const SHA1_NIXBASE32: &str = "mj1l54np5ii9al6g2cjb02n3jxwpf5k0";
    const SHA1_BASE64: &str = "YBZ3eZfDCrAkE89QlWIs15JCg6w=";

    const MD5_SRI: &str = "md5-xIdKiJdECzk9hi2P1FkHPw==";
    const MD5_BASE16: &str = "c4874a8897440b393d862d8fd459073f";
    const MD5_NIXBASE32: &str = "1z0xcx93rdhqykj2s4jy44m1y4";
    const MD5_BASE64: &str = "xIdKiJdECzk9hi2P1FkHPw==";

    const SHA512_SRI: &str = "sha512-q0DQvjVB8HdLungV0T0QsDJS6W6V99u07pmjtDHCFmL9aXGgIBYOOYSKpfMFub4PeHJ7KweJ458STSHpK4857w==";
    const SHA512_BASE16: &str = "ab40d0be3541f0774bba7815d13d10b03252e96e95f7dbb4ee99a3b431c21662fd6971a020160e39848aa5f305b9be0f78727b2b0789e39f124d21e92b8f39ef";
    const SHA512_NIXBASE32: &str = "3pkk3rbx4hls4lzwf4hfavvf9w0zgmr0prsb2l47471c850f5lzsqhnq8qv98wrxssdpxwmdvlm4cmh20yx25bqp95pgw216nzd0h5b";
    const SHA512_BASE64: &str =
        "q0DQvjVB8HdLungV0T0QsDJS6W6V99u07pmjtDHCFmL9aXGgIBYOOYSKpfMFub4PeHJ7KweJ458STSHpK4857w==";

    /// Test parsing a hash without a hash algo specified works if the hash is
    /// in SRI format, and works for all formats if the hash algo is specified.
    #[test]
    fn from_str() {
        let nix_hash_1 = nixhash::from_str(SHA256_SRI, None).expect("must succeed");
        assert_eq!(HashAlgo::Sha256, nix_hash_1.algo);
        assert_eq!(
            vec![
                0xa5, 0xce, 0x9c, 0x15, 0x5e, 0xd0, 0x93, 0x97, 0x61, 0x46, 0x46, 0xc9, 0x71, 0x7f,
                0xc7, 0xcd, 0x94, 0xb1, 0x02, 0x3d, 0x7b, 0x76, 0xb6, 0x18, 0xd4, 0x09, 0xe4, 0xfe,
                0xfd, 0x6e, 0x9d, 0x39
            ],
            nix_hash_1.digest
        );

        // pass the same string, while also specifying the algo
        let nix_hash_2 = nixhash::from_str(SHA256_SRI, Some("sha256")).expect("must succeed");
        // this should be equal to nix_hash_1
        assert_eq!(nix_hash_1, nix_hash_2);

        // parse as base16, while specifying the algo
        let nix_hash_base16 =
            nixhash::from_str(SHA256_BASE16, Some("sha256")).expect("must succeed");
        // this should be equal to nix_hash_1
        assert_eq!(nix_hash_1, nix_hash_base16);

        // parse as nixbase32, while specifying the algo
        let nix_hash_nixbase32 =
            nixhash::from_str(SHA256_NIXBASE32, Some("sha256")).expect("must succeed");
        // this should be equal to nix_hash_1
        assert_eq!(nix_hash_1, nix_hash_nixbase32);

        // parse as base64, while specifying the algo
        let nix_hash_base64 =
            nixhash::from_str(SHA256_BASE64, Some("sha256")).expect("must succeed");
        // this should be equal to nix_hash_1
        assert_eq!(nix_hash_1, nix_hash_base64);
    }

    #[test]
    fn from_str_sha1() {
        let nix_hash_sha1 = nixhash::from_str(SHA1_SRI, None).expect("must succeed");
        assert_eq!(HashAlgo::Sha1, nix_hash_sha1.algo);

        assert_eq!(
            nix_hash_sha1,
            nixhash::from_str(SHA1_BASE16, Some("sha1")).expect("must succeed")
        );
        assert_eq!(
            nix_hash_sha1,
            nixhash::from_str(SHA1_NIXBASE32, Some("sha1")).expect("must succeed")
        );
        assert_eq!(
            nix_hash_sha1,
            nixhash::from_str(SHA1_BASE64, Some("sha1")).expect("must succeed")
        );
    }

    #[test]
    fn from_str_md5() {
        let nix_hash_md5 = nixhash::from_str(MD5_SRI, None).expect("must succeed");
        assert_eq!(HashAlgo::Md5, nix_hash_md5.algo);

        assert_eq!(
            nix_hash_md5,
            nixhash::from_str(MD5_BASE16, Some("md5")).expect("must succeed")
        );
        assert_eq!(
            nix_hash_md5,
            nixhash::from_str(MD5_NIXBASE32, Some("md5")).expect("must succeed")
        );
        assert_eq!(
            nix_hash_md5,
            nixhash::from_str(MD5_BASE64, Some("md5")).expect("must succeed")
        );
    }
    #[test]
    fn from_str_sha512() {
        let nix_hash_sha512 = nixhash::from_str(SHA512_SRI, None).expect("must succeed");
        assert_eq!(HashAlgo::Sha512, nix_hash_sha512.algo);

        assert_eq!(
            nix_hash_sha512,
            nixhash::from_str(SHA512_BASE16, Some("sha512")).expect("must succeed")
        );
        assert_eq!(
            nix_hash_sha512,
            nixhash::from_str(SHA512_NIXBASE32, Some("sha512")).expect("must succeed")
        );
        assert_eq!(
            nix_hash_sha512,
            nixhash::from_str(SHA512_BASE64, Some("sha512")).expect("must succeed")
        );
    }

    /// Test a algo needs to be specified if the hash itself is not SRI.
    #[test]
    fn from_str_algo_missing() {
        nixhash::from_str(SHA256_BASE16, None).expect_err("must fail");
        nixhash::from_str(SHA256_NIXBASE32, None).expect_err("must fail");
        nixhash::from_str(SHA256_BASE64, None).expect_err("must fail");
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
        nixhash::from_str(&broken_base64, Some("sha256")).expect_err("must fail");
    }
}
