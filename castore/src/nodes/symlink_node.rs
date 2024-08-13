use crate::{NamedNode, ValidateNodeError};

/// A SymlinkNode represents a symbolic link in a Directory or at the root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymlinkNode {
    /// The (base)name of the symlink
    name: bytes::Bytes,
    /// The target of the symlink.
    target: bytes::Bytes,
}

impl SymlinkNode {
    pub fn new(name: bytes::Bytes, target: bytes::Bytes) -> Result<Self, ValidateNodeError> {
        if target.is_empty() || target.contains(&b'\0') {
            return Err(ValidateNodeError::InvalidSymlinkTarget(target));
        }
        Ok(Self { name, target })
    }

    pub fn target(&self) -> &bytes::Bytes {
        &self.target
    }

    pub(crate) fn rename(self, name: bytes::Bytes) -> Self {
        Self { name, ..self }
    }
}

impl PartialOrd for SymlinkNode {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SymlinkNode {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.get_name().cmp(other.get_name())
    }
}

impl NamedNode for &SymlinkNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
impl NamedNode for SymlinkNode {
    fn get_name(&self) -> &bytes::Bytes {
        &self.name
    }
}
