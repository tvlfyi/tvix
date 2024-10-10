#![allow(clippy::derive_partial_eq_without_eq, non_snake_case)]
use bstr::ByteSlice;
use bytes::Bytes;
use data_encoding::BASE64;
// https://github.com/hyperium/tonic/issues/1056
use nix_compat::{
    narinfo::{Signature, SignatureError},
    nixhash::{CAHash, NixHash},
    store_path::{self, StorePathRef},
};
use thiserror::Error;
use tvix_castore::DirectoryError;

mod grpc_pathinfoservice_wrapper;

pub use grpc_pathinfoservice_wrapper::GRPCPathInfoServiceWrapper;

tonic::include_proto!("tvix.store.v1");

use tvix_castore::proto as castorepb;

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
    InvalidRootNode(DirectoryError),

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

    /// The narinfo field is missing
    #[error("The narinfo field is missing")]
    NarInfoFieldMissing,

    /// The ca field is invalid
    #[error("The ca field is invalid: {0}")]
    InvalidCaField(ConvertCAError),

    /// The signature at position is invalid
    #[error("The signature at position {0} is invalid: {1}")]
    InvalidSignature(usize, SignatureError),
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

impl From<crate::pathinfoservice::PathInfo> for PathInfo {
    fn from(value: crate::pathinfoservice::PathInfo) -> Self {
        Self {
            node: Some(castorepb::Node::from_name_and_node(
                value.store_path.to_string().into_bytes().into(),
                value.node,
            )),
            references: value
                .references
                .iter()
                .map(|reference| Bytes::copy_from_slice(reference.digest()))
                .collect(),
            narinfo: Some(NarInfo {
                nar_size: value.nar_size,
                nar_sha256: Bytes::copy_from_slice(&value.nar_sha256),
                signatures: value
                    .signatures
                    .iter()
                    .map(|sig| nar_info::Signature {
                        name: sig.name().to_string(),
                        data: Bytes::copy_from_slice(sig.bytes()),
                    })
                    .collect(),
                reference_names: value.references.iter().map(|r| r.to_string()).collect(),
                deriver: value.deriver.as_ref().map(|sp| StorePath {
                    name: (*sp.name()).to_owned(),
                    digest: Bytes::copy_from_slice(sp.digest()),
                }),
                ca: value.ca.as_ref().map(|ca| ca.into()),
            }),
        }
    }
}

impl TryFrom<PathInfo> for crate::pathinfoservice::PathInfo {
    type Error = ValidatePathInfoError;
    fn try_from(value: PathInfo) -> Result<Self, Self::Error> {
        let narinfo = value
            .narinfo
            .ok_or_else(|| ValidatePathInfoError::NarInfoFieldMissing)?;

        // ensure the references have the right number of bytes.
        for (i, reference) in value.references.iter().enumerate() {
            if reference.len() != store_path::DIGEST_SIZE {
                return Err(ValidatePathInfoError::InvalidReferenceDigestLen(
                    i,
                    reference.len(),
                ));
            }
        }

        // ensure the number of references there matches PathInfo.references count.
        if narinfo.reference_names.len() != value.references.len() {
            return Err(ValidatePathInfoError::InconsistentNumberOfReferences(
                value.references.len(),
                narinfo.reference_names.len(),
            ));
        }

        // parse references in reference_names.
        let mut references = vec![];
        for (i, reference_name_str) in narinfo.reference_names.iter().enumerate() {
            // ensure thy parse as (non-absolute) store path
            let reference_names_store_path =
                StorePathRef::from_bytes(reference_name_str.as_bytes()).map_err(|_| {
                    ValidatePathInfoError::InvalidNarinfoReferenceName(
                        i,
                        reference_name_str.to_owned(),
                    )
                })?;

            // ensure their digest matches the one at self.references[i].
            {
                // This is safe, because we ensured the proper length earlier already.
                let reference_digest = value.references[i].to_vec().try_into().unwrap();

                if reference_names_store_path.digest() != &reference_digest {
                    return Err(
                        ValidatePathInfoError::InconsistentNarinfoReferenceNameDigest(
                            i,
                            reference_digest,
                            *reference_names_store_path.digest(),
                        ),
                    );
                } else {
                    references.push(reference_names_store_path.to_owned());
                }
            }
        }

        let nar_sha256_length = narinfo.nar_sha256.len();

        // split value.node into the name and node components
        let (name, node) = value
            .node
            .ok_or_else(|| ValidatePathInfoError::NoNodePresent)?
            .into_name_and_node()
            .map_err(ValidatePathInfoError::InvalidRootNode)?;

        Ok(Self {
            // value.node has a valid name according to the castore model but might not parse to a
            // [StorePath]
            store_path: nix_compat::store_path::StorePath::from_bytes(name.as_ref()).map_err(
                |err| ValidatePathInfoError::InvalidNodeName(name.as_ref().to_vec(), err),
            )?,
            node,
            references,
            nar_size: narinfo.nar_size,
            nar_sha256: narinfo.nar_sha256.to_vec()[..]
                .try_into()
                .map_err(|_| ValidatePathInfoError::InvalidNarSha256DigestLen(nar_sha256_length))?,
            // If the Deriver field is populated, ensure it parses to a
            // [StorePath].
            // We can't check for it to *not* end with .drv, as the .drv files produced by
            // recursive Nix end with multiple .drv suffixes, and only one is popped when
            // converting to this field.
            deriver: narinfo
                .deriver
                .map(|deriver| {
                    nix_compat::store_path::StorePath::from_name_and_digest(
                        &deriver.name,
                        &deriver.digest,
                    )
                    .map_err(ValidatePathInfoError::InvalidDeriverField)
                })
                .transpose()?,
            signatures: narinfo
                .signatures
                .into_iter()
                .enumerate()
                .map(|(i, signature)| {
                    signature.data.to_vec()[..]
                        .try_into()
                        .map_err(|_| {
                            ValidatePathInfoError::InvalidSignature(
                                i,
                                SignatureError::InvalidSignatureLen(signature.data.len()),
                            )
                        })
                        .map(|signature_data| Signature::new(signature.name, signature_data))
                })
                .collect::<Result<Vec<_>, ValidatePathInfoError>>()?,
            ca: narinfo
                .ca
                .as_ref()
                .map(TryFrom::try_from)
                .transpose()
                .map_err(ValidatePathInfoError::InvalidCaField)?,
        })
    }
}
