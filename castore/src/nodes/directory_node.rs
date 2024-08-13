use crate::{B3Digest, NamedNode, ValidateNodeError};

/// A DirectoryNode is a pointer to a [Directory], by its [Directory::digest].
/// It also gives it a `name` and `size`.
/// Such a node is either an element in the [Directory] it itself is contained in,
/// or a standalone root node./
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryNode {
    /// The (base)name of the directory
    name: bytes::Bytes,
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
    pub fn new(name: bytes::Bytes, digest: B3Digest, size: u64) -> Result<Self, ValidateNodeError> {
        Ok(Self { name, digest, size })
    }

    pub fn digest(&self) -> &B3Digest {
        &self.digest
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn rename(self, name: bytes::Bytes) -> Self {
        Self { name, ..self }
    }
}

impl PartialOrd for DirectoryNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DirectoryNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl NamedNode for &DirectoryNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
impl NamedNode for DirectoryNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
