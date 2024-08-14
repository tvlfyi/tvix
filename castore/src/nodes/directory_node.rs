use crate::B3Digest;

/// A DirectoryNode is a pointer to a [Directory], by its [Directory::digest].
/// It also records a`size`.
/// Such a node is either an element in the [Directory] it itself is contained in,
/// or a standalone root node./
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryNode {
    /// The blake3 hash of a Directory message, serialized in protobuf canonical form.
    digest: B3Digest,
    /// Number of child elements in the Directory referred to by `digest`.
    /// Calculated by summing up the numbers of nodes, and for each directory.
    /// its size field. Can be used for inode allocation.
    /// This field is precisely as verifiable as any other Merkle tree edge.
    /// Resolve `digest`, and you can compute it incrementally. Resolve the entire
    /// tree, and you can fully compute it from scratch.
    /// A credulous implementation won't reject an excessive size, but this is
    /// harmless: you'll have some ordinals without nodes. Undersizing is obvious
    /// and easy to reject: you won't have an ordinal for some nodes.
    size: u64,
}

impl DirectoryNode {
    pub fn new(digest: B3Digest, size: u64) -> Self {
        Self { digest, size }
    }

    pub fn digest(&self) -> &B3Digest {
        &self.digest
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}
