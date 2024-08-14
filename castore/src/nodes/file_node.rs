use crate::B3Digest;

/// A FileNode represents a regular or executable file in a Directory or at the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileNode {
    /// The blake3 digest of the file contents
    digest: B3Digest,

    /// The file content size
    size: u64,

    /// Whether the file is executable
    executable: bool,
}

impl FileNode {
    pub fn new(digest: B3Digest, size: u64, executable: bool) -> Self {
        Self {
            digest,
            size,
            executable,
        }
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
}
