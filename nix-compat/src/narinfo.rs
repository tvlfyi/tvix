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
    pub fn parse(input: &'a str) -> Option<Self> {
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
            let (tag, val) = line.split_once(':')?;
            let val = val.strip_prefix(' ')?;

            match tag {
                "StorePath" => {
                    let val = val.strip_prefix("/nix/store/")?;
                    let val = StorePathRef::from_bytes(val.as_bytes()).ok()?;

                    if store_path.replace(val).is_some() {
                        return None;
                    }
                }
                "URL" => {
                    if val.is_empty() {
                        return None;
                    }

                    if url.replace(val).is_some() {
                        return None;
                    }
                }
                "Compression" => {
                    if val.is_empty() {
                        return None;
                    }

                    if compression.replace(val).is_some() {
                        return None;
                    }
                }
                "FileHash" => {
                    let val = val.strip_prefix("sha256:")?;
                    let val = nixbase32::decode_fixed::<32>(val).ok()?;

                    if file_hash.replace(val).is_some() {
                        return None;
                    }
                }
                "FileSize" => {
                    let val = val.parse::<u64>().ok()?;

                    if file_size.replace(val).is_some() {
                        return None;
                    }
                }
                "NarHash" => {
                    let val = val.strip_prefix("sha256:")?;
                    let val = nixbase32::decode_fixed::<32>(val).ok()?;

                    if nar_hash.replace(val).is_some() {
                        return None;
                    }
                }
                "NarSize" => {
                    let val = val.parse::<u64>().ok()?;

                    if nar_size.replace(val).is_some() {
                        return None;
                    }
                }
                "References" => {
                    let val: Vec<StorePathRef> = if !val.is_empty() {
                        let mut prev = "";
                        val.split(' ')
                            .map(|s| {
                                if mem::replace(&mut prev, s) < s {
                                    StorePathRef::from_bytes(s.as_bytes()).ok()
                                } else {
                                    // references are out of order
                                    None
                                }
                            })
                            .collect::<Option<_>>()?
                    } else {
                        vec![]
                    };

                    if references.replace(val).is_some() {
                        return None;
                    }
                }
                "System" => {
                    if val.is_empty() {
                        return None;
                    }

                    if system.replace(val).is_some() {
                        return None;
                    }
                }
                "Deriver" => {
                    let val = StorePathRef::from_bytes(val.as_bytes()).ok()?;

                    if !val.name().ends_with(".drv") {
                        return None;
                    }

                    if deriver.replace(val).is_some() {
                        return None;
                    }
                }
                "Sig" => {
                    let val = Signature::parse(val)?;

                    signatures.push(val);
                }
                "CA" => {
                    let val = parse_ca(val)?;

                    if ca.replace(val).is_some() {
                        return None;
                    }
                }
                _ => {
                    // unknown field, ignore
                }
            }
        }

        Some(NarInfo {
            store_path: store_path?,
            nar_hash: nar_hash?,
            nar_size: nar_size?,
            references: references?,
            signatures,
            ca,
            system,
            deriver,
            url: url?,
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
    pub fn parse(input: &'a str) -> Option<Signature<'a>> {
        let (name, bytes64) = input.split_once(':')?;

        let mut buf = [0; 66];
        let mut bytes = [0; 64];
        match BASE64.decode_mut(bytes64.as_bytes(), &mut buf) {
            Ok(64) => {
                bytes.copy_from_slice(&buf[..64]);
            }
            _ => {
                return None;
            }
        }

        Some(Signature { name, bytes })
    }

    pub fn name(&self) -> &'a str {
        self.name
    }

    pub fn bytes(&self) -> &[u8; 64] {
        &self.bytes
    }
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
            if let Some(digest) = s.strip_prefix("r:sha256:") {
                let digest = nixbase32::decode_fixed(digest).ok()?;
                Some(CAHash::Nar(NixHash::Sha256(digest)))
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

#[cfg(test)]
mod test {
    use lazy_static::lazy_static;
    use pretty_assertions::assert_eq;
    use std::{io, str};

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
}
