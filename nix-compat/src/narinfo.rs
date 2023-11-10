//! NAR info files describe a store path in a traditional Nix binary cache.
//! Over the wire, they are formatted as "Key: value" pairs separated by newlines.
//!
//! It contains four kinds of information:
//! 1. the description of the store path itself
//!    * store path prefix, digest, and name
//!    * NAR hash and size
//!    * references
//! 2. authenticity information
//!    * zero or more signatures over that description
//!    * an optional [CAHash] for content-addressed paths (fixed outputs, sources, and derivations)
//! 3. derivation metadata
//!    * deriver (the derivation that produced this path)
//!    * system (the system value of that derivation)
//! 4. cache-specific information
//!    * URL of the compressed NAR, relative to the NAR info file
//!    * compression algorithm used for the NAR
//!    * hash and size of the compressed NAR

use data_encoding::BASE64;
use std::{
    fmt::{self, Display},
    mem,
};

use crate::{
    nixbase32,
    nixhash::{CAHash, NixHash},
    store_path::StorePathRef,
};

#[derive(Debug)]
pub struct NarInfo<'a> {
    // core (authenticated, but unverified here)
    /// Store path described by this [NarInfo]
    pub store_path: StorePathRef<'a>,
    /// SHA-256 digest of the NAR file
    pub nar_hash: [u8; 32],
    /// Size of the NAR file in bytes
    pub nar_size: u64,
    /// Store paths known to be referenced by the contents
    pub references: Vec<StorePathRef<'a>>,
    // authenticity
    /// Ed25519 signature over the path fingerprint
    pub signatures: Vec<Signature<'a>>,
    /// Content address (for content-defined paths)
    pub ca: Option<CAHash>,
    // derivation metadata
    /// Nix system triple of [deriver]
    pub system: Option<&'a str>,
    /// Store path of the derivation that produced this
    pub deriver: Option<StorePathRef<'a>>,
    // cache-specific untrusted metadata
    /// Relative URL of the compressed NAR file
    pub url: &'a str,
    /// Compression method of the NAR file
    /// TODO(edef): default this to bzip2, and have None mean "none" (uncompressed)
    pub compression: Option<&'a str>,
    /// SHA-256 digest of the file at `url`
    pub file_hash: Option<[u8; 32]>,
    /// Size of the file at `url` in bytes
    pub file_size: Option<u64>,
}

impl<'a> NarInfo<'a> {
    pub fn parse(input: &'a str) -> Result<Self, Error> {
        let mut store_path = None;
        let mut url = None;
        let mut compression = None;
        let mut file_hash = None;
        let mut file_size = None;
        let mut nar_hash = None;
        let mut nar_size = None;
        let mut references = None;
        let mut system = None;
        let mut deriver = None;
        let mut signatures = vec![];
        let mut ca = None;

        for line in input.lines() {
            let (tag, val) = line
                .split_once(':')
                .ok_or_else(|| Error::InvalidLine(line.to_string()))?;

            let val = val
                .strip_prefix(' ')
                .ok_or_else(|| Error::InvalidLine(line.to_string()))?;

            match tag {
                "StorePath" => {
                    let val = val
                        .strip_prefix("/nix/store/")
                        .ok_or(Error::InvalidStorePath(
                            crate::store_path::Error::MissingStoreDir,
                        ))?;
                    let val = StorePathRef::from_bytes(val.as_bytes())
                        .map_err(Error::InvalidStorePath)?;

                    if store_path.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "URL" => {
                    if val.is_empty() {
                        return Err(Error::EmptyField(tag.to_string()));
                    }

                    if url.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "Compression" => {
                    if val.is_empty() {
                        return Err(Error::EmptyField(tag.to_string()));
                    }

                    if compression.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "FileHash" => {
                    let val = val
                        .strip_prefix("sha256:")
                        .ok_or_else(|| Error::MissingPrefixForHash(tag.to_string()))?;
                    let val = nixbase32::decode_fixed::<32>(val)
                        .map_err(|e| Error::UnableToDecodeHash(tag.to_string(), e))?;

                    if file_hash.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "FileSize" => {
                    let val = val
                        .parse::<u64>()
                        .map_err(|_| Error::UnableToParseSize(tag.to_string(), val.to_string()))?;

                    if file_size.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "NarHash" => {
                    let val = val
                        .strip_prefix("sha256:")
                        .ok_or_else(|| Error::MissingPrefixForHash(tag.to_string()))?;

                    let val = nixbase32::decode_fixed::<32>(val)
                        .map_err(|e| Error::UnableToDecodeHash(tag.to_string(), e))?;

                    if nar_hash.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "NarSize" => {
                    let val = val
                        .parse::<u64>()
                        .map_err(|_| Error::UnableToParseSize(tag.to_string(), val.to_string()))?;

                    if nar_size.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "References" => {
                    let val: Vec<StorePathRef> = if !val.is_empty() {
                        let mut prev = "";
                        val.split(' ')
                            .enumerate()
                            .map(|(i, s)| {
                                if mem::replace(&mut prev, s) < s {
                                    StorePathRef::from_bytes(s.as_bytes())
                                        .map_err(|err| Error::InvalidReference(i, err))
                                } else {
                                    // references are out of order
                                    Err(Error::OutOfOrderReference(i))
                                }
                            })
                            .collect::<Result<_, _>>()?
                    } else {
                        vec![]
                    };

                    if references.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "System" => {
                    if val.is_empty() {
                        return Err(Error::EmptyField(tag.to_string()));
                    }

                    if system.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "Deriver" => {
                    let val = StorePathRef::from_bytes(val.as_bytes())
                        .map_err(Error::InvalidDeriverStorePath)?;

                    if !val.name().ends_with(".drv") {
                        return Err(Error::InvalidDeriverStorePathMissingSuffix);
                    }

                    if deriver.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                "Sig" => {
                    let val = Signature::parse(val)
                        .map_err(|e| Error::UnableToParseSignature(signatures.len(), e))?;

                    signatures.push(val);
                }
                "CA" => {
                    let val =
                        parse_ca(val).ok_or_else(|| Error::UnableToParseCA(val.to_string()))?;

                    if ca.replace(val).is_some() {
                        return Err(Error::DuplicateField(tag.to_string()));
                    }
                }
                _ => {
                    // unknown field, ignore
                }
            }
        }

        Ok(NarInfo {
            store_path: store_path.ok_or(Error::MissingField("StorePath"))?,
            nar_hash: nar_hash.ok_or(Error::MissingField("NarHash"))?,
            nar_size: nar_size.ok_or(Error::MissingField("NarSize"))?,
            references: references.ok_or(Error::MissingField("References"))?,
            signatures,
            ca,
            system,
            deriver,
            url: url.ok_or(Error::MissingField("URL"))?,
            compression,
            file_hash,
            file_size,
        })
    }
}

impl Display for NarInfo<'_> {
    fn fmt(&self, w: &mut fmt::Formatter) -> fmt::Result {
        writeln!(w, "StorePath: /nix/store/{}", self.store_path)?;
        writeln!(w, "URL: {}", self.url)?;

        if let Some(compression) = self.compression {
            writeln!(w, "Compression: {compression}")?;
        }

        if let Some(file_hash) = self.file_hash {
            writeln!(w, "FileHash: {}", fmt_hash(&NixHash::Sha256(file_hash)))?;
        }

        if let Some(file_size) = self.file_size {
            writeln!(w, "FileSize: {file_size}")?;
        }

        writeln!(w, "NarHash: {}", fmt_hash(&NixHash::Sha256(self.nar_hash)))?;
        writeln!(w, "NarSize: {}", self.nar_size)?;

        write!(w, "References:")?;
        if self.references.is_empty() {
            write!(w, " ")?;
        } else {
            for path in &self.references {
                write!(w, " {path}")?;
            }
        }
        writeln!(w)?;

        if let Some(deriver) = &self.deriver {
            writeln!(w, "Deriver: {deriver}")?;
        }

        if let Some(system) = self.system {
            writeln!(w, "System: {system}")?;
        }

        for sig in &self.signatures {
            writeln!(w, "Sig: {sig}")?;
        }

        if let Some(ca) = &self.ca {
            writeln!(w, "CA: {}", fmt_ca(ca))?;
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct Signature<'a> {
    name: &'a str,
    bytes: [u8; 64],
}

impl<'a> Signature<'a> {
    pub fn parse(input: &'a str) -> Result<Signature<'a>, SignatureError> {
        let (name, bytes64) = input
            .split_once(':')
            .ok_or(SignatureError::MissingSeparator)?;

        let mut buf = [0; 66];
        let mut bytes = [0; 64];
        match BASE64.decode_mut(bytes64.as_bytes(), &mut buf) {
            Ok(64) => {
                bytes.copy_from_slice(&buf[..64]);
            }
            Ok(n) => return Err(SignatureError::InvalidSignatureLen(n)),
            // keeping DecodePartial gets annoying lifetime-wise
            Err(_) => return Err(SignatureError::DecodeError(input.to_string())),
        }

        Ok(Signature { name, bytes })
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn bytes(&self) -> &[u8; 64] {
        &self.bytes
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("Missing separator")]
    MissingSeparator,
    #[error("Invalid signature len: {0}")]
    InvalidSignatureLen(usize),
    #[error("Unable to base64-decode signature: {0}")]
    DecodeError(String),
}

impl Display for Signature<'_> {
    fn fmt(&self, w: &mut fmt::Formatter) -> fmt::Result {
        write!(w, "{}:{}", self.name, BASE64.encode(&self.bytes))
    }
}

pub fn parse_ca(s: &str) -> Option<CAHash> {
    let (tag, s) = s.split_once(':')?;

    match tag {
        "text" => {
            let digest = s.strip_prefix("sha256:")?;
            let digest = nixbase32::decode_fixed(digest).ok()?;
            Some(CAHash::Text(digest))
        }
        "fixed" => {
            if let Some(s) = s.strip_prefix("r:") {
                parse_hash(s).map(CAHash::Nar)
            } else {
                parse_hash(s).map(CAHash::Flat)
            }
        }
        _ => None,
    }
}

#[allow(non_camel_case_types)]
struct fmt_ca<'a>(&'a CAHash);

impl Display for fmt_ca<'_> {
    fn fmt(&self, w: &mut fmt::Formatter) -> fmt::Result {
        match self.0 {
            CAHash::Flat(h) => {
                write!(w, "fixed:{}", fmt_hash(h))
            }
            &CAHash::Text(d) => {
                write!(w, "text:{}", fmt_hash(&NixHash::Sha256(d)))
            }
            CAHash::Nar(h) => {
                write!(w, "fixed:r:{}", fmt_hash(h))
            }
        }
    }
}

fn parse_hash(s: &str) -> Option<NixHash> {
    let (tag, digest) = s.split_once(':')?;

    (match tag {
        "md5" => nixbase32::decode_fixed(digest).map(NixHash::Md5),
        "sha1" => nixbase32::decode_fixed(digest).map(NixHash::Sha1),
        "sha256" => nixbase32::decode_fixed(digest).map(NixHash::Sha256),
        "sha512" => nixbase32::decode_fixed(digest)
            .map(Box::new)
            .map(NixHash::Sha512),
        _ => return None,
    })
    .ok()
}

#[allow(non_camel_case_types)]
struct fmt_hash<'a>(&'a NixHash);

impl Display for fmt_hash<'_> {
    fn fmt(&self, w: &mut fmt::Formatter) -> fmt::Result {
        let (tag, digest) = match self.0 {
            NixHash::Md5(d) => ("md5", &d[..]),
            NixHash::Sha1(d) => ("sha1", &d[..]),
            NixHash::Sha256(d) => ("sha256", &d[..]),
            NixHash::Sha512(d) => ("sha512", &d[..]),
        };

        write!(w, "{tag}:{}", nixbase32::encode(digest))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("duplicate field: {0}")]
    DuplicateField(String),

    #[error("missing field: {0}")]
    MissingField(&'static str),

    #[error("invalid line: {0}")]
    InvalidLine(String),

    #[error("invalid StorePath: {0}")]
    InvalidStorePath(crate::store_path::Error),

    #[error("field {0} may not be empty string")]
    EmptyField(String),

    #[error("invalid {0}: {1}")]
    UnableToParseSize(String, String),

    #[error("unable to parse #{0} reference: {1}")]
    InvalidReference(usize, crate::store_path::Error),

    #[error("reference at {0} is out of order")]
    OutOfOrderReference(usize),

    #[error("invalid Deriver store path: {0}")]
    InvalidDeriverStorePath(crate::store_path::Error),

    #[error("invalid Deriver store path, must end with .drv")]
    InvalidDeriverStorePathMissingSuffix,

    #[error("missing prefix for {0}")]
    MissingPrefixForHash(String),

    #[error("unable to decode {0}: {1}")]
    UnableToDecodeHash(String, nixbase32::Nixbase32DecodeError),

    #[error("unable to parse signature #{0}: {1}")]
    UnableToParseSignature(usize, SignatureError),

    #[error("unable to parse CA field: {0}")]
    UnableToParseCA(String),
}

#[cfg(test)]
mod test {
    use hex_literal::hex;
    use lazy_static::lazy_static;
    use pretty_assertions::assert_eq;
    use std::{io, str};

    use crate::nixhash::{CAHash, NixHash};

    use super::NarInfo;

    lazy_static! {
        static ref CASES: &'static [&'static str] = {
            let data = zstd::decode_all(io::Cursor::new(include_bytes!("../testdata/narinfo.zst")))
                .unwrap();
            let data = str::from_utf8(Vec::leak(data)).unwrap();
            Vec::leak(
                data.split_inclusive("\n\n")
                    .map(|s| s.strip_suffix('\n').unwrap())
                    .collect::<Vec<_>>(),
            )
        };
    }

    #[test]
    fn roundtrip() {
        for &input in *CASES {
            let parsed = NarInfo::parse(input).expect("should parse");
            let output = format!("{parsed}");
            assert_eq!(input, output, "should roundtrip");
        }
    }

    #[test]
    fn ca_nar_hash_sha1() {
        let parsed = NarInfo::parse(
            r#"StorePath: /nix/store/k20pahypzvr49fy82cw5sx72hdfg3qcr-texlive-hyphenex-37354
URL: nar/0i5biw0g01514llhfswxy6xfav8lxxdq1xg6ik7hgsqbpw0f06yi.nar.xz
Compression: xz
FileHash: sha256:0i5biw0g01514llhfswxy6xfav8lxxdq1xg6ik7hgsqbpw0f06yi
FileSize: 7120
NarHash: sha256:0h1bm4sj1cnfkxgyhvgi8df1qavnnv94sd0v09wcrm971602shfg
NarSize: 22552
References: 
Sig: cache.nixos.org-1:u01BybwQhyI5H1bW1EIWXssMDhDDIvXOG5uh8Qzgdyjz6U1qg6DHhMAvXZOUStIj6X5t4/ufFgR8i3fjf0bMAw==
CA: fixed:r:sha1:1ak1ymbmsfx7z8kh09jzkr3a4dvkrfjw
"#).expect("should parse");

        assert_eq!(
            parsed.ca,
            Some(CAHash::Nar(NixHash::Sha1(hex!(
                "5cba3c77236ae4f9650270a27fbad375551fa60a"
            ))))
        );
    }
}
