#![allow(clippy::derive_partial_eq_without_eq, non_snake_case)]
use bytes::Bytes;
use data_encoding::BASE64;
// https://github.com/hyperium/tonic/issues/1056
use nix_compat::{
    nixhash::{CAHash, NixHash},
    store_path,
};
use thiserror::Error;
use tvix_castore::proto::{self as castorepb, NamedNode, ValidateNodeError};

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
    #[error("Failed to parse {0:?} as StorePath: {1}")]
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
/// On success, this returns the parsed [store_path::StorePath].
/// On error, it returns an error generated from the supplied constructor.
fn parse_node_name_root<E>(
    name: &[u8],
    err: fn(Vec<u8>, store_path::Error) -> E,
) -> Result<store_path::StorePath, E> {
    match store_path::StorePath::from_bytes(name) {
        Ok(np) => Ok(np),
        Err(e) => Err(err(name.to_vec(), e)),
    }
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
                    store_path::StorePath::from_name_and_digest(
                        deriver.name.clone(),
                        &deriver.digest,
                    )
                    .map_err(ValidatePathInfoError::InvalidDeriverField)?;
                }
            }
        }

        // Ensure there is a (root) node present, and it properly parses to a [store_path::StorePath].
        let root_nix_path = match &self.node {
            None | Some(castorepb::Node { node: None }) => {
                Err(ValidatePathInfoError::NoNodePresent)?
            }
            Some(castorepb::Node { node: Some(node) }) => {
                node.validate()
                    .map_err(ValidatePathInfoError::InvalidRootNode)?;
                // parse the name of the node itself and return
                parse_node_name_root(node.get_name(), ValidatePathInfoError::InvalidNodeName)?
            }
        };

        // return the root nix path
        Ok(root_nix_path)
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

        let ca = value.ca.as_ref().map(|ca_hash| nar_info::Ca {
            r#type: match ca_hash {
                CAHash::Flat(NixHash::Md5(_)) => nar_info::ca::Hash::FlatMd5.into(),
                CAHash::Flat(NixHash::Sha1(_)) => nar_info::ca::Hash::FlatSha1.into(),
                CAHash::Flat(NixHash::Sha256(_)) => nar_info::ca::Hash::FlatSha256.into(),
                CAHash::Flat(NixHash::Sha512(_)) => nar_info::ca::Hash::FlatSha512.into(),
                CAHash::Nar(NixHash::Md5(_)) => nar_info::ca::Hash::NarMd5.into(),
                CAHash::Nar(NixHash::Sha1(_)) => nar_info::ca::Hash::NarSha1.into(),
                CAHash::Nar(NixHash::Sha256(_)) => nar_info::ca::Hash::NarSha256.into(),
                CAHash::Nar(NixHash::Sha512(_)) => nar_info::ca::Hash::NarSha512.into(),
                CAHash::Text(_) => nar_info::ca::Hash::TextSha256.into(),
            },
            digest: Bytes::copy_from_slice(ca_hash.digest().digest_as_bytes()),
        });

        NarInfo {
            nar_size: value.nar_size,
            nar_sha256: Bytes::copy_from_slice(&value.nar_hash),
            signatures,
            reference_names: value.references.iter().map(|r| r.to_string()).collect(),
            deriver: value.deriver.as_ref().map(|sp| StorePath {
                name: sp.name().to_owned(),
                digest: Bytes::copy_from_slice(sp.digest()),
            }),
            ca,
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
