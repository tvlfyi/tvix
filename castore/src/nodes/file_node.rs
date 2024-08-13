use crate::{B3Digest, NamedNode, ValidateNodeError};

/// A FileNode represents a regular or executable file in a Directory or at the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    /// The (base)name of the file
    name: bytes::Bytes,

    /// The blake3 digest of the file contents
    digest: B3Digest,

    /// The file content size
    size: u64,

    /// Whether the file is executable
    executable: bool,
}

impl FileNode {
    pub fn new(
        name: bytes::Bytes,
        digest: B3Digest,
        size: u64,
        executable: bool,
    ) -> Result<Self, ValidateNodeError> {
        Ok(Self {
            name,
            digest,
            size,
            executable,
        })
    }

    pub fn digest(&self) -> &B3Digest {
        &self.digest
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn executable(&self) -> bool {
        self.executable
    }

    pub fn rename(self, name: bytes::Bytes) -> Self {
        Self { name, ..self }
    }
}

impl PartialOrd for FileNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for FileNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl NamedNode for &FileNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
impl NamedNode for FileNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
