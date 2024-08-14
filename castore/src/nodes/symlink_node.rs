use crate::ValidateNodeError;

/// A SymlinkNode represents a symbolic link in a Directory or at the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymlinkNode {
    /// The target of the symlink.
    target: bytes::Bytes,
}

impl SymlinkNode {
    pub fn new(target: bytes::Bytes) -> Result<Self, ValidateNodeError> {
        if target.is_empty() || target.contains(&b'\0') {
            return Err(ValidateNodeError::InvalidSymlinkTarget(target));
        }
        Ok(Self { target })
    }

    pub fn target(&self) -> &bytes::Bytes {
        &self.target
    }
}
