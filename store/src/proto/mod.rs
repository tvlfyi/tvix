#![allow(clippy::derive_partial_eq_without_eq, non_snake_case)]
// https://github.com/hyperium/tonic/issues/1056
use nix_compat::store_path::{self, StorePath};
use thiserror::Error;
use tvix_castore::{proto as castorepb, B3Digest, B3_LEN};

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
    /// No node present
    #[error("No node present")]
    NoNodePresent(),

    /// Invalid node name encountered.
    #[error("Failed to parse {0:?} as StorePath: {1}")]
    InvalidNodeName(Vec<u8>, store_path::Error),

    /// The digest the (root) node refers to has invalid length.
    #[error("Invalid Digest length: expected {}, got {}", B3_LEN, .0)]
    InvalidNodeDigestLen(usize),

    /// The number of references in the narinfo.reference_names field does not match
    /// the number of references in the .references field.
    #[error("Inconsistent Number of References: {0} (references) vs {0} (narinfo)")]
    InconsistentNumberOfReferences(usize, usize),
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
        // If there is a narinfo field populated, ensure the number of references there
        // matches PathInfo.references count.
        if let Some(narinfo) = &self.narinfo {
            if narinfo.reference_names.len() != self.references.len() {
                return Err(ValidatePathInfoError::InconsistentNumberOfReferences(
                    narinfo.reference_names.len(),
                    self.references.len(),
                ));
            }
        }
        // FUTUREWORK: parse references in reference_names. ensure they start
        // with storeDir, and use the same digest as in self.references.

        // Ensure there is a (root) node present, and it properly parses to a [StorePath].
        let root_nix_path = match &self.node {
            None => {
                return Err(ValidatePathInfoError::NoNodePresent());
            }
            Some(castorepb::Node { node }) => match node {
                None => {
                    return Err(ValidatePathInfoError::NoNodePresent());
                }
                Some(castorepb::node::Node::Directory(directory_node)) => {
                    // ensure the digest has the appropriate size.
                    if TryInto::<B3Digest>::try_into(directory_node.digest.clone()).is_err() {
                        return Err(ValidatePathInfoError::InvalidNodeDigestLen(
                            directory_node.digest.len(),
                        ));
                    }

                    // parse the name
                    parse_node_name_root(
                        &directory_node.name,
                        ValidatePathInfoError::InvalidNodeName,
                    )?
                }
                Some(castorepb::node::Node::File(file_node)) => {
                    // ensure the digest has the appropriate size.
                    if TryInto::<B3Digest>::try_into(file_node.digest.clone()).is_err() {
                        return Err(ValidatePathInfoError::InvalidNodeDigestLen(
                            file_node.digest.len(),
                        ));
                    }

                    // parse the name
                    parse_node_name_root(&file_node.name, ValidatePathInfoError::InvalidNodeName)?
                }
                Some(castorepb::node::Node::Symlink(symlink_node)) => {
                    // parse the name
                    parse_node_name_root(
                        &symlink_node.name,
                        ValidatePathInfoError::InvalidNodeName,
                    )?
                }
            },
        };

        // return the root nix path
        Ok(root_nix_path)
    }
}
