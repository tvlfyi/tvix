#![allow(clippy::derive_partial_eq_without_eq, non_snake_case)]
use data_encoding::BASE64;
// https://github.com/hyperium/tonic/issues/1056
use nix_compat::store_path::{self, StorePath};
use thiserror::Error;
use tvix_castore::{
    proto::{self as castorepb, NamedNode},
    B3Digest, B3_LEN,
};

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
    NoNodePresent(),

    /// Invalid node name encountered.
    #[error("Failed to parse {0:?} as StorePath: {1}")]
    InvalidNodeName(Vec<u8>, store_path::Error),

    /// The digest the (root) node refers to has invalid length.
    #[error("Invalid Digest length: expected {}, got {}", B3_LEN, .0)]
    InvalidNodeDigestLen(usize),

    /// The digest in narinfo.nar_sha256 has an invalid len.
    #[error("Invalid narinfo.nar_sha256 length: expected {}, got {}", 32, .0)]
    InvalidNarSha256DigestLen(usize),

    /// The number of references in the narinfo.reference_names field does not match
    /// the number of references in the .references field.
    #[error("Inconsistent Number of References: {0} (references) vs {1} (narinfo)")]
    InconsistentNumberOfReferences(usize, usize),

    /// A string in narinfo.reference_names does not parse to a StorePath.
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
}

/// Parses a root node name.
///
/// On success, this returns the parsed [StorePath].
/// On error, it returns an error generated from the supplied constructor.
fn parse_node_name_root<E>(
    name: &[u8],
    err: fn(Vec<u8>, store_path::Error) -> E,
) -> Result<StorePath, E> {
    match StorePath::from_bytes(name) {
        Ok(np) => Ok(np),
        Err(e) => Err(err(name.to_vec(), e)),
    }
}

impl PathInfo {
    /// validate performs some checks on the PathInfo struct,
    /// Returning either a [StorePath] of the root node, or a
    /// [ValidatePathInfoError].
    pub fn validate(&self) -> Result<StorePath, ValidatePathInfoError> {
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
                let reference_names_store_path =
                    StorePath::from_bytes(reference_name_str.as_bytes()).map_err(|_| {
                        ValidatePathInfoError::InvalidNarinfoReferenceName(
                            i,
                            reference_name_str.to_owned(),
                        )
                    })?;

                // ensure their digest matches the one at self.references[i].
                {
                    // This is safe, because we ensured the proper length earlier already.
                    let reference_digest = self.references[i].to_vec().try_into().unwrap();

                    if reference_names_store_path.digest != reference_digest {
                        return Err(
                            ValidatePathInfoError::InconsistentNarinfoReferenceNameDigest(
                                i,
                                reference_digest,
                                reference_names_store_path.digest,
                            ),
                        );
                    }
                }
            }
        }

        // Ensure there is a (root) node present, and it properly parses to a [StorePath].
        let root_nix_path = match &self.node {
            None => {
                return Err(ValidatePathInfoError::NoNodePresent());
            }
            Some(castorepb::Node { node }) => match node {
                None => {
                    return Err(ValidatePathInfoError::NoNodePresent());
                }
                Some(node) => {
                    match node {
                        // for a directory root node, ensure the digest has the appropriate size.
                        castorepb::node::Node::Directory(directory_node) => {
                            if TryInto::<B3Digest>::try_into(directory_node.digest.clone()).is_err()
                            {
                                return Err(ValidatePathInfoError::InvalidNodeDigestLen(
                                    directory_node.digest.len(),
                                ));
                            }
                        }
                        // for a file root node, ensure the digest has the appropriate size.
                        castorepb::node::Node::File(file_node) => {
                            // ensure the digest has the appropriate size.
                            if TryInto::<B3Digest>::try_into(file_node.digest.clone()).is_err() {
                                return Err(ValidatePathInfoError::InvalidNodeDigestLen(
                                    file_node.digest.len(),
                                ));
                            }
                        }
                        // nothing to do specifically for symlinks
                        castorepb::node::Node::Symlink(_) => {}
                    }
                    // parse the name of the node itself and return
                    parse_node_name_root(&node.get_name(), ValidatePathInfoError::InvalidNodeName)?
                }
            },
        };

        // return the root nix path
        Ok(root_nix_path)
    }
}
