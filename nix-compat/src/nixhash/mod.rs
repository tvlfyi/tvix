use crate::nixbase32;
use data_encoding::{BASE64, BASE64_NOPAD, HEXLOWER};
use thiserror;

mod algos;
mod ca_hash;

pub use algos::HashAlgo;
pub use ca_hash::CAHash;

/// NixHash represents hashes known by Nix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum NixHash {
    Md5([u8; 16]),
    Sha1([u8; 20]),
    Sha256([u8; 32]),
    Sha512(Box<[u8; 64]>),
}

/// convenience Result type for all nixhash parsing Results.
pub type Result<V> = std::result::Result<V, Error>;

impl NixHash {
    /// returns the algo as [HashAlgo].
    pub fn algo(&self) -> HashAlgo {
        match self {
            NixHash::Md5(_) => HashAlgo::Md5,
            NixHash::Sha1(_) => HashAlgo::Sha1,
            NixHash::Sha256(_) => HashAlgo::Sha256,
            NixHash::Sha512(_) => HashAlgo::Sha512,
        }
    }

    /// returns the digest as variable-length byte slice.
    pub fn digest_as_bytes(&self) -> &[u8] {
        match self {
            NixHash::Md5(digest) => digest,
            NixHash::Sha1(digest) => digest,
            NixHash::Sha256(digest) => digest,
            NixHash::Sha512(digest) => digest.as_ref(),
        }
    }

    /// Formats a [NixHash] in the Nix default hash format,
    /// which is the algo, followed by a colon, then the lower hex encoded digest.
    pub fn to_nix_hash_string(&self) -> String {
        format!(
            "{}:{}",
            self.algo(),
            HEXLOWER.encode(self.digest_as_bytes())
        )
    }
}

impl TryFrom<(HashAlgo, &[u8])> for NixHash {
    type Error = Error;

    /// Constructs a new [NixHash] by specifying [HashAlgo] and digest.
    /// It can fail if the passed digest length doesn't match what's expected for
    /// the passed algo.
    fn try_from(value: (HashAlgo, &[u8])) -> Result<Self> {
        let (algo, digest) = value;
        from_algo_and_digest(algo, digest)
    }
}

/// Constructs a new [NixHash] by specifying [HashAlgo] and digest.
/// It can fail if the passed digest length doesn't match what's expected for
/// the passed algo.
pub fn from_algo_and_digest(algo: HashAlgo, digest: &[u8]) -> Result<NixHash> {
    if digest.len() != algo.digest_length() {
        return Err(Error::InvalidEncodedDigestLength(digest.len(), algo));
    }

    Ok(match algo {
        HashAlgo::Md5 => NixHash::Md5(digest.try_into().unwrap()),
        HashAlgo::Sha1 => NixHash::Sha1(digest.try_into().unwrap()),
        HashAlgo::Sha256 => NixHash::Sha256(digest.try_into().unwrap()),
        HashAlgo::Sha512 => NixHash::Sha512(Box::new(digest.try_into().unwrap())),
    })
}

/// Errors related to NixHash construction.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
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
    ConflictingHashAlgos(HashAlgo, HashAlgo),
    #[error("missing inline hash algo, but no externally-specified algo: {0}")]
    MissingInlineHashAlgo(String),
}

/// Nix allows specifying hashes in various encodings, and magically just
/// derives the encoding.
/// This function parses strings to a NixHash.
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
pub fn from_str(s: &str, algo_str: Option<&str>) -> Result<NixHash> {
    // if algo_str is some, parse or bail out
    let algo: Option<HashAlgo> = if let Some(algo_str) = algo_str {
        Some(algo_str.try_into()?)
    } else {
        None
    };

    // Peek at the beginning of the string to detect SRI hashes.
    if s.starts_with("sha1-")
        || s.starts_with("sha256-")
        || s.starts_with("sha512-")
        || s.starts_with("md5-")
    {
        let parsed_nixhash = from_sri_str(s)?;

        // ensure the algo matches with what has been passed externally, if so.
        if let Some(algo) = algo {
            if algo != parsed_nixhash.algo() {
                return Err(Error::ConflictingHashAlgos(algo, parsed_nixhash.algo()));
            }
        }
        return Ok(parsed_nixhash);
    }

    // Peek at the beginning again to see if it's a Nix Hash
    if s.starts_with("sha1:")
        || s.starts_with("sha256:")
        || s.starts_with("sha512:")
        || s.starts_with("md5:")
    {
        let parsed_nixhash = from_nix_str(s)?;
        // ensure the algo matches with what has been passed externally, if so.
        if let Some(algo) = algo {
            if algo != parsed_nixhash.algo() {
                return Err(Error::ConflictingHashAlgos(algo, parsed_nixhash.algo()));
            }
        }
        return Ok(parsed_nixhash);
    }

    // Neither of these, assume a bare digest, so there MUST be an externally-passed algo.
    match algo {
        // Fail if there isn't.
        None => Err(Error::MissingInlineHashAlgo(s.to_string())),
        Some(algo) => decode_digest(s.as_bytes(), algo),
    }
}

/// Parses a Nix hash string ($algo:$digest) to a NixHash.
pub fn from_nix_str(s: &str) -> Result<NixHash> {
    if let Some(rest) = s.strip_prefix("sha1:") {
        decode_digest(rest.as_bytes(), HashAlgo::Sha1)
    } else if let Some(rest) = s.strip_prefix("sha256:") {
        decode_digest(rest.as_bytes(), HashAlgo::Sha256)
    } else if let Some(rest) = s.strip_prefix("sha512:") {
        decode_digest(rest.as_bytes(), HashAlgo::Sha512)
    } else if let Some(rest) = s.strip_prefix("md5:") {
        decode_digest(rest.as_bytes(), HashAlgo::Md5)
    } else {
        Err(Error::InvalidAlgo(s.to_string()))
    }
}

/// Parses a Nix SRI string to a NixHash.
/// Contrary to the SRI spec, Nix doesn't support SRI strings with multiple hashes,
/// only supports sha256 and sha512 from the spec, and supports sha1 and md5
/// additionally.
/// It also accepts SRI strings where the base64 has an with invalid padding.
pub fn from_sri_str(s: &str) -> Result<NixHash> {
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

    // decode the digest and algo into a [NixHash]
    match decode_digest(encoded_digest.as_bytes(), algo) {
        // If decoding was successful, pass along
        Ok(nixhash) => Ok(nixhash),
        // For SRI hashes (only), BASE64_NOPAD is also tolerated,
        // so try to parse for this, too.
        // NOTE: As of now, we reject SRI hashes containing additional
        // characters (which upstream Nix seems to simply truncate), as
        // there's no occurence of this is in nixpkgs.
        // It most likely should also be a bug in Nix.
        Err(Error::InvalidEncodedDigestLength(digest_len, hash_algo)) => {
            if encoded_digest.len() == BASE64_NOPAD.encode_len(algo.digest_length()) {
                let digest = BASE64_NOPAD
                    .decode(encoded_digest.as_bytes())
                    .map_err(Error::InvalidBase64Encoding)?;
                Ok(from_algo_and_digest(algo, &digest).unwrap())
            } else {
                Err(Error::InvalidEncodedDigestLength(digest_len, hash_algo))?
            }
        }
        Err(e) => Err(e)?,
    }
}

/// Decode a plain digest depending on the hash algo specified externally.
/// hexlower, nixbase32 and base64 encodings are supported - the encoding is
/// inferred from the input length.
fn decode_digest(s: &[u8], algo: HashAlgo) -> Result<NixHash> {
    // for the chosen hash algo, calculate the expected (decoded) digest length
    // (as bytes)
    let digest = if s.len() == HEXLOWER.encode_len(algo.digest_length()) {
        HEXLOWER
            .decode(s.as_ref())
            .map_err(Error::InvalidBase16Encoding)?
    } else if s.len() == nixbase32::encode_len(algo.digest_length()) {
        nixbase32::decode(s).map_err(Error::InvalidBase32Encoding)?
    } else if s.len() == BASE64.encode_len(algo.digest_length()) {
        BASE64
            .decode(s.as_ref())
            .map_err(Error::InvalidBase64Encoding)?
    } else {
        Err(Error::InvalidEncodedDigestLength(s.len(), algo))?
    };

    Ok(from_algo_and_digest(algo, &digest).unwrap())
}

#[cfg(test)]
mod tests {
    use crate::{
        nixbase32,
        nixhash::{self, HashAlgo, NixHash},
    };
    use data_encoding::{BASE64, BASE64_NOPAD, HEXLOWER};
    use test_case::test_case;
    const DIGEST_SHA1: [u8; 20] = [
        0x60, 0x16, 0x77, 0x79, 0x97, 0xc3, 0x0a, 0xb0, 0x24, 0x13, 0xcf, 0x50, 0x95, 0x62, 0x2c,
        0xd7, 0x92, 0x42, 0x83, 0xac,
    ];

    const DIGEST_SHA256: [u8; 32] = [
        0xa5, 0xce, 0x9c, 0x15, 0x5e, 0xd0, 0x93, 0x97, 0x61, 0x46, 0x46, 0xc9, 0x71, 0x7f, 0xc7,
        0xcd, 0x94, 0xb1, 0x02, 0x3d, 0x7b, 0x76, 0xb6, 0x18, 0xd4, 0x09, 0xe4, 0xfe, 0xfd, 0x6e,
        0x9d, 0x39,
    ];

    const DIGEST_SHA512: [u8; 64] = [
        0xab, 0x40, 0xd0, 0xbe, 0x35, 0x41, 0xf0, 0x77, 0x4b, 0xba, 0x78, 0x15, 0xd1, 0x3d, 0x10,
        0xb0, 0x32, 0x52, 0xe9, 0x6e, 0x95, 0xf7, 0xdb, 0xb4, 0xee, 0x99, 0xa3, 0xb4, 0x31, 0xc2,
        0x16, 0x62, 0xfd, 0x69, 0x71, 0xa0, 0x20, 0x16, 0x0e, 0x39, 0x84, 0x8a, 0xa5, 0xf3, 0x05,
        0xb9, 0xbe, 0x0f, 0x78, 0x72, 0x7b, 0x2b, 0x07, 0x89, 0xe3, 0x9f, 0x12, 0x4d, 0x21, 0xe9,
        0x2b, 0x8f, 0x39, 0xef,
    ];
    const DIGEST_MD5: [u8; 16] = [
        0xc4, 0x87, 0x4a, 0x88, 0x97, 0x44, 0x0b, 0x39, 0x3d, 0x86, 0x2d, 0x8f, 0xd4, 0x59, 0x07,
        0x3f,
    ];

    fn to_base16(digest: &[u8]) -> String {
        HEXLOWER.encode(digest)
    }

    fn to_nixbase32(digest: &[u8]) -> String {
        nixbase32::encode(digest)
    }

    fn to_base64(digest: &[u8]) -> String {
        BASE64.encode(digest)
    }

    fn to_base64_nopad(digest: &[u8]) -> String {
        BASE64_NOPAD.encode(digest)
    }

    // TODO
    fn make_nixhash(algo: &HashAlgo, digest_encoded: String) -> String {
        format!("{}:{}", algo, digest_encoded)
    }
    fn make_sri_string(algo: &HashAlgo, digest_encoded: String) -> String {
        format!("{}-{}", algo, digest_encoded)
    }

    /// Test parsing a hash string in various formats, and also when/how the out-of-band algo is needed.
    #[test_case(&NixHash::Sha1(DIGEST_SHA1); "sha1")]
    #[test_case(&NixHash::Sha256(DIGEST_SHA256); "sha256")]
    #[test_case(&NixHash::Sha512(Box::new(DIGEST_SHA512)); "sha512")]
    #[test_case(&NixHash::Md5(DIGEST_MD5); "md5")]
    fn from_str(expected_hash: &NixHash) {
        let algo = &expected_hash.algo();
        let digest = expected_hash.digest_as_bytes();
        // parse SRI
        {
            // base64 without out-of-band algo
            let s = make_sri_string(algo, to_base64(digest));
            let h = nixhash::from_str(&s, None).expect("must succeed");
            assert_eq!(expected_hash, &h);

            // base64 with out-of-band-algo
            let s = make_sri_string(algo, to_base64(digest));
            let h = nixhash::from_str(&s, Some(&expected_hash.algo().to_string()))
                .expect("must succeed");
            assert_eq!(expected_hash, &h);

            // base64_nopad without out-of-band algo
            let s = make_sri_string(algo, to_base64_nopad(digest));
            let h = nixhash::from_str(&s, None).expect("must succeed");
            assert_eq!(expected_hash, &h);

            // base64_nopad with out-of-band-algo
            let s = make_sri_string(algo, to_base64_nopad(digest));
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, &h);
        }

        // parse plain base16. should succeed with algo out-of-band, but fail without.
        {
            let s = to_base16(digest);
            nixhash::from_str(&s, None).expect_err("must fail");
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, &h);
        }

        // parse plain nixbase32. should succeed with algo out-of-band, but fail without.
        {
            let s = to_nixbase32(digest);
            nixhash::from_str(&s, None).expect_err("must fail");
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, &h);
        }

        // parse plain base64. should succeed with algo out-of-band, but fail without.
        {
            let s = to_base64(digest);
            nixhash::from_str(&s, None).expect_err("must fail");
            let h = nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed");
            assert_eq!(expected_hash, &h);
        }

        // parse Nix hash strings
        {
            // base16. should succeed with both algo out-of-band and in-band.
            {
                let s = make_nixhash(algo, to_base16(digest));
                assert_eq!(
                    expected_hash,
                    &nixhash::from_str(&s, None).expect("must succeed")
                );
                assert_eq!(
                    expected_hash,
                    &nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed")
                );
            }
            // nixbase32. should succeed with both algo out-of-band and in-band.
            {
                let s = make_nixhash(algo, to_nixbase32(digest));
                assert_eq!(
                    expected_hash,
                    &nixhash::from_str(&s, None).expect("must succeed")
                );
                assert_eq!(
                    expected_hash,
                    &nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed")
                );
            }
            // base64. should succeed with both algo out-of-band and in-band.
            {
                let s = make_nixhash(algo, to_base64(digest));
                assert_eq!(
                    expected_hash,
                    &nixhash::from_str(&s, None).expect("must succeed")
                );
                assert_eq!(
                    expected_hash,
                    &nixhash::from_str(&s, Some(&algo.to_string())).expect("must succeed")
                );
            }
        }
    }

    /// Test parsing an SRI hash via the [nixhash::from_sri_str] method.
    #[test]
    fn from_sri_str() {
        let nix_hash = nixhash::from_sri_str("sha256-pc6cFV7Qk5dhRkbJcX/HzZSxAj17drYY1Ank/v1unTk=")
            .expect("must succeed");

        assert_eq!(HashAlgo::Sha256, nix_hash.algo());
        assert_eq!(
            vec![
                0xa5, 0xce, 0x9c, 0x15, 0x5e, 0xd0, 0x93, 0x97, 0x61, 0x46, 0x46, 0xc9, 0x71, 0x7f,
                0xc7, 0xcd, 0x94, 0xb1, 0x02, 0x3d, 0x7b, 0x76, 0xb6, 0x18, 0xd4, 0x09, 0xe4, 0xfe,
                0xfd, 0x6e, 0x9d, 0x39
            ]
            .as_slice(),
            nix_hash.digest_as_bytes()
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
        assert_eq!(&expected_digest, &nix_hash.digest_as_bytes());

        // not passing hash algo out of band should succeed
        let nix_hash =
            nixhash::from_str(&format!("sha256-{}", &broken_base64), None).expect("must succeed");
        assert_eq!(&expected_digest, &nix_hash.digest_as_bytes());

        // not passing SRI, but hash algo out of band should fail
        nixhash::from_str(broken_base64, Some("sha256")).expect_err("must fail");
    }
}
