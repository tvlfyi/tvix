#![allow(clippy::derive_partial_eq_without_eq, non_snake_case)]
use bstr::ByteSlice;
use bytes::Bytes;
use data_encoding::BASE64;
// https://github.com/hyperium/tonic/issues/1056
use nix_compat::{
    narinfo::Flags,
    nixhash::{CAHash, NixHash},
    store_path::{self, StorePathRef},
};
use thiserror::Error;
use tvix_castore::{NamedNode, ValidateNodeError};

mod grpc_pathinfoservice_wrapper;

pub use grpc_pathinfoservice_wrapper::GRPCPathInfoServiceWrapper;

tonic::include_proto!("tvix.store.v1");

#[cfg(feature = "tonic-reflection")]
/// Compiled file descriptors for implementing [gRPC
/// reflection](https://github.com/grpc/grpc/blob/master/doc/server-reflection.md) with e.g.
/// [`tonic_reflection`](https://docs.rs/tonic-reflection).
pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("tvix.store.v1");

#[cfg(test)]
mod tests;

/// Errors that can occur during the validation of PathInfo messages.
#[derive(Debug, Error, PartialEq)]
pub enum ValidatePathInfoError {
    /// Invalid length of a reference
    #[error("Invalid length of digest at position {}, expected {}, got {}", .0, store_path::DIGEST_SIZE, .1)]
    InvalidReferenceDigestLen(usize, usize),

    /// No node present
    #[error("No node present")]
    NoNodePresent,

    /// Node fails validation
    #[error("Invalid root node: {:?}", .0.to_string())]
    InvalidRootNode(ValidateNodeError),

    /// Invalid node name encountered. Root nodes in PathInfos have more strict name requirements
    #[error("Failed to parse {} as StorePath: {1}", .0.to_str_lossy())]
    InvalidNodeName(Vec<u8>, store_path::Error),

    /// The digest in narinfo.nar_sha256 has an invalid len.
    #[error("Invalid narinfo.nar_sha256 length: expected {}, got {}", 32, .0)]
    InvalidNarSha256DigestLen(usize),

    /// The number of references in the narinfo.reference_names field does not match
    /// the number of references in the .references field.
    #[error("Inconsistent Number of References: {0} (references) vs {1} (narinfo)")]
    InconsistentNumberOfReferences(usize, usize),

    /// A string in narinfo.reference_names does not parse to a [store_path::StorePath].
    #[error("Invalid reference_name at position {0}: {1}")]
    InvalidNarinfoReferenceName(usize, String),

    /// The digest in the parsed `.narinfo.reference_names[i]` does not match
    /// the one in `.references[i]`.`
    #[error("digest in reference_name at position {} does not match digest in PathInfo, expected {}, got {}", .0, BASE64.encode(.1), BASE64.encode(.2))]
    InconsistentNarinfoReferenceNameDigest(
        usize,
        [u8; store_path::DIGEST_SIZE],
        [u8; store_path::DIGEST_SIZE],
    ),

    /// The deriver field is invalid.
    #[error("deriver field is invalid: {0}")]
    InvalidDeriverField(store_path::Error),
}

/// Parses a root node name.
///
/// On success, this returns the parsed [store_path::StorePathRef].
/// On error, it returns an error generated from the supplied constructor.
fn parse_node_name_root<E>(
    name: &[u8],
    err: fn(Vec<u8>, store_path::Error) -> E,
) -> Result<store_path::StorePathRef<'_>, E> {
    store_path::StorePathRef::from_bytes(name).map_err(|e| err(name.to_vec(), e))
}

impl PathInfo {
    /// validate performs some checks on the PathInfo struct,
    /// Returning either a [store_path::StorePath] of the root node, or a
    /// [ValidatePathInfoError].
    pub fn validate(&self) -> Result<store_path::StorePath, ValidatePathInfoError> {
        // ensure the references have the right number of bytes.
        for (i, reference) in self.references.iter().enumerate() {
            if reference.len() != store_path::DIGEST_SIZE {
                return Err(ValidatePathInfoError::InvalidReferenceDigestLen(
                    i,
                    reference.len(),
                ));
            }
        }

        // If there is a narinfo field populated…
        if let Some(narinfo) = &self.narinfo {
            // ensure the nar_sha256 digest has the correct length.
            if narinfo.nar_sha256.len() != 32 {
                return Err(ValidatePathInfoError::InvalidNarSha256DigestLen(
                    narinfo.nar_sha256.len(),
                ));
            }

            // ensure the number of references there matches PathInfo.references count.
            if narinfo.reference_names.len() != self.references.len() {
                return Err(ValidatePathInfoError::InconsistentNumberOfReferences(
                    self.references.len(),
                    narinfo.reference_names.len(),
                ));
            }

            // parse references in reference_names.
            for (i, reference_name_str) in narinfo.reference_names.iter().enumerate() {
                // ensure thy parse as (non-absolute) store path
                let reference_names_store_path = store_path::StorePath::from_bytes(
                    reference_name_str.as_bytes(),
                )
                .map_err(|_| {
                    ValidatePathInfoError::InvalidNarinfoReferenceName(
                        i,
                        reference_name_str.to_owned(),
                    )
                })?;

                // ensure their digest matches the one at self.references[i].
                {
                    // This is safe, because we ensured the proper length earlier already.
                    let reference_digest = self.references[i].to_vec().try_into().unwrap();

                    if reference_names_store_path.digest() != &reference_digest {
                        return Err(
                            ValidatePathInfoError::InconsistentNarinfoReferenceNameDigest(
                                i,
                                reference_digest,
                                *reference_names_store_path.digest(),
                            ),
                        );
                    }
                }

                // If the Deriver field is populated, ensure it parses to a
                // [store_path::StorePath].
                // We can't check for it to *not* end with .drv, as the .drv files produced by
                // recursive Nix end with multiple .drv suffixes, and only one is popped when
                // converting to this field.
                if let Some(deriver) = &narinfo.deriver {
                    store_path::StorePathRef::from_name_and_digest(&deriver.name, &deriver.digest)
                        .map_err(ValidatePathInfoError::InvalidDeriverField)?;
                }
            }
        }

        // Ensure there is a (root) node present, and it properly parses to a [store_path::StorePath].
        let root_nix_path = match &self.node {
            None => Err(ValidatePathInfoError::NoNodePresent)?,
            Some(node) => {
                // TODO save result somewhere
                let node: tvix_castore::Node = node
                    .try_into()
                    .map_err(ValidatePathInfoError::InvalidRootNode)?;
                // parse the name of the node itself and return
                parse_node_name_root(node.get_name(), ValidatePathInfoError::InvalidNodeName)?
                    .to_owned()
            }
        };

        // return the root nix path
        Ok(root_nix_path)
    }

    /// With self and its store path name, this reconstructs a
    /// [nix_compat::narinfo::NarInfo<'_>].
    /// It can be used to validate Signatures, or get back a (sparse) NarInfo
    /// struct to prepare writing it out.
    ///
    /// It assumes self to be validated first, and will only return None if the
    /// `narinfo` field is unpopulated.
    ///
    /// It does very little allocation (a Vec each for `signatures` and
    /// `references`), the rest points to data owned elsewhere.
    ///
    /// Keep in mind this is not able to reconstruct all data present in the
    /// NarInfo<'_>, as some of it is not stored at all:
    /// - the `system`, `file_hash` and `file_size` fields are set to `None`.
    /// - the URL is set to an empty string.
    /// - Compression is set to "none"
    ///
    /// If you want to render it out to a string and be able to parse it back
    /// in, at least URL *must* be set again.
    pub fn to_narinfo<'a>(
        &'a self,
        store_path: store_path::StorePathRef<'a>,
    ) -> Option<nix_compat::narinfo::NarInfo<'_>> {
        let narinfo = &self.narinfo.as_ref()?;

        Some(nix_compat::narinfo::NarInfo {
            flags: Flags::empty(),
            store_path,
            nar_hash: narinfo
                .nar_sha256
                .as_ref()
                .try_into()
                .expect("invalid narhash"),
            nar_size: narinfo.nar_size,
            references: narinfo
                .reference_names
                .iter()
                .map(|ref_name| {
                    // This shouldn't pass validation
                    StorePathRef::from_bytes(ref_name.as_bytes()).expect("invalid reference")
                })
                .collect(),
            signatures: narinfo
                .signatures
                .iter()
                .map(|sig| {
                    nix_compat::narinfo::Signature::new(
                        &sig.name,
                        // This shouldn't pass validation
                        sig.data[..].try_into().expect("invalid signature len"),
                    )
                })
                .collect(),
            ca: narinfo
                .ca
                .as_ref()
                .map(|ca| ca.try_into().expect("invalid ca")),
            system: None,
            deriver: narinfo.deriver.as_ref().map(|deriver| {
                StorePathRef::from_name_and_digest(&deriver.name, &deriver.digest)
                    .expect("invalid deriver")
            }),
            url: "",
            compression: Some("none"),
            file_hash: None,
            file_size: None,
        })
    }
}

/// Errors that can occur when converting from a [nar_info::Ca] to a (stricter)
/// [nix_compat::nixhash::CAHash].
#[derive(Debug, Error, PartialEq)]
pub enum ConvertCAError {
    /// Invalid length of a reference
    #[error("Invalid digest length '{0}' for type {1}")]
    InvalidReferenceDigestLen(usize, &'static str),
    /// Unknown Hash type
    #[error("Unknown hash type: {0}")]
    UnknownHashType(i32),
}

impl TryFrom<&nar_info::Ca> for nix_compat::nixhash::CAHash {
    type Error = ConvertCAError;

    fn try_from(value: &nar_info::Ca) -> Result<Self, Self::Error> {
        Ok(match value.r#type {
            typ if typ == nar_info::ca::Hash::NarSha256 as i32 => {
                Self::Nar(NixHash::Sha256(value.digest[..].try_into().map_err(
                    |_| ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "NarSha256"),
                )?))
            }
            typ if typ == nar_info::ca::Hash::NarSha1 as i32 => {
                Self::Nar(NixHash::Sha1(value.digest[..].try_into().map_err(
                    |_| ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "NarSha1"),
                )?))
            }
            typ if typ == nar_info::ca::Hash::NarSha512 as i32 => Self::Nar(NixHash::Sha512(
                Box::new(value.digest[..].try_into().map_err(|_| {
                    ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "NarSha512")
                })?),
            )),
            typ if typ == nar_info::ca::Hash::NarMd5 as i32 => {
                Self::Nar(NixHash::Md5(value.digest[..].try_into().map_err(|_| {
                    ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "NarMd5")
                })?))
            }
            typ if typ == nar_info::ca::Hash::TextSha256 as i32 => {
                Self::Text(value.digest[..].try_into().map_err(|_| {
                    ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "TextSha256")
                })?)
            }
            typ if typ == nar_info::ca::Hash::FlatSha1 as i32 => {
                Self::Flat(NixHash::Sha1(value.digest[..].try_into().map_err(
                    |_| ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "FlatSha1"),
                )?))
            }
            typ if typ == nar_info::ca::Hash::FlatMd5 as i32 => {
                Self::Flat(NixHash::Md5(value.digest[..].try_into().map_err(|_| {
                    ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "FlatMd5")
                })?))
            }
            typ if typ == nar_info::ca::Hash::FlatSha256 as i32 => {
                Self::Flat(NixHash::Sha256(value.digest[..].try_into().map_err(
                    |_| ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "FlatSha256"),
                )?))
            }
            typ if typ == nar_info::ca::Hash::FlatSha512 as i32 => Self::Flat(NixHash::Sha512(
                Box::new(value.digest[..].try_into().map_err(|_| {
                    ConvertCAError::InvalidReferenceDigestLen(value.digest.len(), "FlatSha512")
                })?),
            )),
            typ => return Err(ConvertCAError::UnknownHashType(typ)),
        })
    }
}

impl From<&nix_compat::nixhash::CAHash> for nar_info::ca::Hash {
    fn from(value: &nix_compat::nixhash::CAHash) -> Self {
        match value {
            CAHash::Flat(NixHash::Md5(_)) => nar_info::ca::Hash::FlatMd5,
            CAHash::Flat(NixHash::Sha1(_)) => nar_info::ca::Hash::FlatSha1,
            CAHash::Flat(NixHash::Sha256(_)) => nar_info::ca::Hash::FlatSha256,
            CAHash::Flat(NixHash::Sha512(_)) => nar_info::ca::Hash::FlatSha512,
            CAHash::Nar(NixHash::Md5(_)) => nar_info::ca::Hash::NarMd5,
            CAHash::Nar(NixHash::Sha1(_)) => nar_info::ca::Hash::NarSha1,
            CAHash::Nar(NixHash::Sha256(_)) => nar_info::ca::Hash::NarSha256,
            CAHash::Nar(NixHash::Sha512(_)) => nar_info::ca::Hash::NarSha512,
            CAHash::Text(_) => nar_info::ca::Hash::TextSha256,
        }
    }
}

impl From<&nix_compat::nixhash::CAHash> for nar_info::Ca {
    fn from(value: &nix_compat::nixhash::CAHash) -> Self {
        nar_info::Ca {
            r#type: Into::<nar_info::ca::Hash>::into(value) as i32,
            digest: value.hash().digest_as_bytes().to_vec().into(),
        }
    }
}

impl From<&nix_compat::narinfo::NarInfo<'_>> for NarInfo {
    /// Converts from a NarInfo (returned from the NARInfo parser) to the proto-
    /// level NarInfo struct.
    fn from(value: &nix_compat::narinfo::NarInfo<'_>) -> Self {
        let signatures = value
            .signatures
            .iter()
            .map(|sig| nar_info::Signature {
                name: sig.name().to_string(),
                data: Bytes::copy_from_slice(sig.bytes()),
            })
            .collect();

        NarInfo {
            nar_size: value.nar_size,
            nar_sha256: Bytes::copy_from_slice(&value.nar_hash),
            signatures,
            reference_names: value.references.iter().map(|r| r.to_string()).collect(),
            deriver: value.deriver.as_ref().map(|sp| StorePath {
                name: sp.name().to_owned(),
                digest: Bytes::copy_from_slice(sp.digest()),
            }),
            ca: value.ca.as_ref().map(|ca| ca.into()),
        }
    }
}

impl From<&nix_compat::narinfo::NarInfo<'_>> for PathInfo {
    /// Converts from a NarInfo (returned from the NARInfo parser) to a PathInfo
    /// struct with the node set to None.
    fn from(value: &nix_compat::narinfo::NarInfo<'_>) -> Self {
        Self {
            node: None,
            references: value
                .references
                .iter()
                .map(|x| Bytes::copy_from_slice(x.digest()))
                .collect(),
            narinfo: Some(value.into()),
        }
    }
}
