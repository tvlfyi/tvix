use std::collections::HashSet;
use tracing::warn;

use super::Directory;
use crate::{B3Digest, Node};

pub trait OrderValidator {
    /// Update the order validator's state with the directory
    /// Returns whether the directory was accepted
    fn add_directory(&mut self, directory: &Directory) -> bool;
}

#[derive(Default)]
/// Validates that newly introduced directories are already referenced from
/// the root via existing directories.
/// Commonly used when _receiving_ a directory closure _from_ a store.
pub struct RootToLeavesValidator {
    /// Only used to remember the root node, not for validation
    expected_digests: HashSet<B3Digest>,
}

impl RootToLeavesValidator {
    /// Use to validate the root digest of the closure upon receiving the first
    /// directory.
    pub fn new_with_root_digest(root_digest: B3Digest) -> Self {
        let mut this = Self::default();
        this.expected_digests.insert(root_digest);
        this
    }

    /// Checks if a directory is in-order based on its digest.
    ///
    /// Particularly useful when receiving directories in canonical protobuf
    /// encoding, so that directories not connected to the root can be rejected
    /// without parsing.
    ///
    /// After parsing, the directory must be passed to `add_directory_unchecked`
    /// to add its children to the list of expected digests.
    pub fn digest_allowed(&self, digest: &B3Digest) -> bool {
        self.expected_digests.is_empty() // we don't know the root node; allow any
            || self.expected_digests.contains(digest)
    }

    /// Update the order validator's state with the directory
    pub fn add_directory_unchecked(&mut self, directory: &Directory) {
        // No initial root was specified and this is the first directory
        if self.expected_digests.is_empty() {
            self.expected_digests.insert(directory.digest());
        }

        // Allow the children to appear next
        for (_, node) in directory.nodes() {
            if let Node::Directory { digest, .. } = node {
                self.expected_digests.insert(digest.clone());
            }
        }
    }
}

impl OrderValidator for RootToLeavesValidator {
    fn add_directory(&mut self, directory: &Directory) -> bool {
        if !self.digest_allowed(&directory.digest()) {
            return false;
        }
        self.add_directory_unchecked(directory);
        true
    }
}

#[derive(Default)]
/// Validates that newly uploaded directories only reference directories which
/// have already been introduced.
/// Commonly used when _uploading_ a directory closure _to_ a store.
pub struct LeavesToRootValidator {
    /// This is empty in the beginning, and gets filled as leaves and intermediates are
    /// inserted
    allowed_references: HashSet<B3Digest>,
}

impl OrderValidator for LeavesToRootValidator {
    fn add_directory(&mut self, directory: &Directory) -> bool {
        let digest = directory.digest();

        for (_, node) in directory.nodes() {
            if let Node::Directory {
                digest: subdir_node_digest,
                ..
            } = node
            {
                if !self.allowed_references.contains(subdir_node_digest) {
                    warn!(
                        directory.digest = %digest,
                        subdirectory.digest = %subdir_node_digest,
                        "unexpected directory reference"
                    );
                    return false;
                }
            }
        }

        self.allowed_references.insert(digest.clone());

        true
    }
}

#[cfg(test)]
mod tests {
    use super::{LeavesToRootValidator, RootToLeavesValidator};
    use crate::directoryservice::order_validator::OrderValidator;
    use crate::directoryservice::Directory;
    use crate::fixtures::{DIRECTORY_A, DIRECTORY_B, DIRECTORY_C};
    use rstest::rstest;

    #[rstest]
    /// Uploading an empty directory should succeed.
    #[case::empty_directory(&[&*DIRECTORY_A], false)]
    /// Uploading A, then B (referring to A) should succeed.
    #[case::simple_closure(&[&*DIRECTORY_A, &*DIRECTORY_B], false)]
    /// Uploading A, then A, then C (referring to A twice) should succeed.
    /// We pretend to be a dumb client not deduping directories.
    #[case::same_child(&[&*DIRECTORY_A, &*DIRECTORY_A, &*DIRECTORY_C], false)]
    /// Uploading A, then C (referring to A twice) should succeed.
    #[case::same_child_dedup(&[&*DIRECTORY_A, &*DIRECTORY_C], false)]
    /// Uploading A, then C (referring to A twice), then B (itself referring to A) should fail during close,
    /// as B itself would be left unconnected.
    #[case::unconnected_node(&[&*DIRECTORY_A, &*DIRECTORY_C, &*DIRECTORY_B], false)]
    /// Uploading B (referring to A) should fail immediately, because A was never uploaded.
    #[case::dangling_pointer(&[&*DIRECTORY_B], true)]
    fn leaves_to_root(
        #[case] directories_to_upload: &[&Directory],
        #[case] exp_fail_upload_last: bool,
    ) {
        let mut validator = LeavesToRootValidator::default();
        let len_directories_to_upload = directories_to_upload.len();

        for (i, d) in directories_to_upload.iter().enumerate() {
            let resp = validator.add_directory(d);
            if i == len_directories_to_upload - 1 && exp_fail_upload_last {
                assert!(!resp, "expect last put to fail");

                // We don't really care anymore what finalize() would return, as
                // the add() failed.
                return;
            } else {
                assert!(resp, "expect put to succeed");
            }
        }
    }

    #[rstest]
    /// Downloading an empty directory should succeed.
    #[case::empty_directory(&*DIRECTORY_A, &[&*DIRECTORY_A], false)]
    /// Downlading B, then A (referenced by B) should succeed.
    #[case::simple_closure(&*DIRECTORY_B, &[&*DIRECTORY_B, &*DIRECTORY_A], false)]
    /// Downloading C (referring to A twice), then A should succeed.
    #[case::same_child_dedup(&*DIRECTORY_C, &[&*DIRECTORY_C, &*DIRECTORY_A], false)]
    /// Downloading C, then B (both referring to A but not referring to each other) should fail immediately as B has no connection to C (the root)
    #[case::unconnected_node(&*DIRECTORY_C, &[&*DIRECTORY_C, &*DIRECTORY_B], true)]
    /// Downloading B (specified as the root) but receiving A instead should fail immediately, because A has no connection to B (the root).
    #[case::dangling_pointer(&*DIRECTORY_B, &[&*DIRECTORY_A], true)]
    fn root_to_leaves(
        #[case] root: &Directory,
        #[case] directories_to_upload: &[&Directory],
        #[case] exp_fail_upload_last: bool,
    ) {
        let mut validator = RootToLeavesValidator::new_with_root_digest(root.digest());
        let len_directories_to_upload = directories_to_upload.len();

        for (i, d) in directories_to_upload.iter().enumerate() {
            let resp1 = validator.digest_allowed(&d.digest());
            let resp = validator.add_directory(d);
            assert_eq!(
                resp1, resp,
                "digest_allowed should return the same value as add_directory"
            );
            if i == len_directories_to_upload - 1 && exp_fail_upload_last {
                assert!(!resp, "expect last put to fail");

                // We don't really care anymore what finalize() would return, as
                // the add() failed.
                return;
            } else {
                assert!(resp, "expect put to succeed");
            }
        }
    }
}
