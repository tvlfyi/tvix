use crate::{
    proto, B3Digest, DirectoryNode, FileNode, NamedNode, Node, SymlinkNode, ValidateDirectoryError,
    ValidateNodeError,
};

/// A Directory can contain Directory, File or Symlink nodes.
/// Each of these nodes have a name attribute, which is the basename in that
/// directory and node type specific attributes.
/// While a Node by itself may have any name, the names of Directory entries:
///  - MUST not contain slashes or null bytes
///  - MUST not be '.' or '..'
///  - MUST be unique across all three lists
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    nodes: Vec<Node>,
}

impl Directory {
    pub fn new() -> Self {
        Directory { nodes: vec![] }
    }

    /// The size of a directory is the number of all regular and symlink elements,
    /// the number of directory elements, and their size fields.
    pub fn size(&self) -> u64 {
        // It's impossible to create a Directory where the size overflows, because we
        // check before every add() that the size won't overflow.
        (self.nodes.len() as u64) + self.directories().map(|e| e.size()).sum::<u64>()
    }

    /// Calculates the digest of a Directory, which is the blake3 hash of a
    /// Directory protobuf message, serialized in protobuf canonical form.
    pub fn digest(&self) -> B3Digest {
        proto::Directory::from(self.clone()).digest()
    }

    /// Allows iterating over all nodes (directories, files and symlinks)
    /// ordered by their name.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> + Send + Sync + '_ {
        self.nodes.iter()
    }

    /// Allows iterating over the FileNode entries of this directory
    /// ordered by their name
    pub fn files(&self) -> impl Iterator<Item = &FileNode> + Send + Sync + '_ {
        self.nodes.iter().filter_map(|node| match node {
            Node::File(n) => Some(n),
            _ => None,
        })
    }

    /// Allows iterating over the subdirectories of this directory
    /// ordered by their name
    pub fn directories(&self) -> impl Iterator<Item = &DirectoryNode> + Send + Sync + '_ {
        self.nodes.iter().filter_map(|node| match node {
            Node::Directory(n) => Some(n),
            _ => None,
        })
    }

    /// Allows iterating over the SymlinkNode entries of this directory
    /// ordered by their name
    pub fn symlinks(&self) -> impl Iterator<Item = &SymlinkNode> + Send + Sync + '_ {
        self.nodes.iter().filter_map(|node| match node {
            Node::Symlink(n) => Some(n),
            _ => None,
        })
    }

    /// Checks a Node name for validity as a directory entry
    /// We disallow slashes, null bytes, '.', '..' and the empty string.
    pub(crate) fn validate_node_name(name: &[u8]) -> Result<(), ValidateNodeError> {
        if name.is_empty()
            || name == b".."
            || name == b"."
            || name.contains(&0x00)
            || name.contains(&b'/')
        {
            Err(ValidateNodeError::InvalidName(name.to_owned().into()))
        } else {
            Ok(())
        }
    }

    /// Adds the specified [Node] to the [Directory], preserving sorted entries.
    ///
    /// Inserting an element that already exists with the same name in the directory will yield an
    /// error.
    /// Inserting an element will validate that its name fulfills the stricter requirements for
    /// directory entries and yield an error if it is not.
    pub fn add(&mut self, node: Node) -> Result<(), ValidateDirectoryError> {
        Self::validate_node_name(node.get_name())
            .map_err(|e| ValidateDirectoryError::InvalidNode(node.get_name().clone().into(), e))?;

        // Check that the even after adding this new directory entry, the size calculation will not
        // overflow
        // FUTUREWORK: add some sort of batch add interface which only does this check once with
        // all the to-be-added entries
        checked_sum([
            self.size(),
            1,
            match node {
                Node::Directory(ref dir) => dir.size(),
                _ => 0,
            },
        ])
        .ok_or(ValidateDirectoryError::SizeOverflow)?;

        // This assumes the [Directory] is sorted, since we don't allow accessing the nodes list
        // directly and all previous inserts should have been in-order
        let pos = match self
            .nodes
            .binary_search_by_key(&node.get_name(), |n| n.get_name())
        {
            Err(pos) => pos, // There is no node with this name; good!
            Ok(_) => {
                return Err(ValidateDirectoryError::DuplicateName(
                    node.get_name().to_vec(),
                ))
            }
        };

        self.nodes.insert(pos, node);
        Ok(())
    }
}

fn checked_sum(iter: impl IntoIterator<Item = u64>) -> Option<u64> {
    iter.into_iter().try_fold(0u64, |acc, i| acc.checked_add(i))
}

#[cfg(test)]
mod test {
    use super::{Directory, DirectoryNode, FileNode, Node, SymlinkNode};
    use crate::fixtures::DUMMY_DIGEST;
    use crate::ValidateDirectoryError;

    #[test]
    fn add_nodes_to_directory() {
        let mut d = Directory::new();

        d.add(Node::Directory(
            DirectoryNode::new("b".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();
        d.add(Node::Directory(
            DirectoryNode::new("a".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();
        d.add(Node::Directory(
            DirectoryNode::new("z".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();

        d.add(Node::File(
            FileNode::new("f".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
        ))
        .unwrap();
        d.add(Node::File(
            FileNode::new("c".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
        ))
        .unwrap();
        d.add(Node::File(
            FileNode::new("g".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
        ))
        .unwrap();

        d.add(Node::Symlink(
            SymlinkNode::new("t".into(), "a".into()).unwrap(),
        ))
        .unwrap();
        d.add(Node::Symlink(
            SymlinkNode::new("o".into(), "a".into()).unwrap(),
        ))
        .unwrap();
        d.add(Node::Symlink(
            SymlinkNode::new("e".into(), "a".into()).unwrap(),
        ))
        .unwrap();

        // Convert to proto struct and back to ensure we are not generating any invalid structures
        crate::Directory::try_from(crate::proto::Directory::from(d))
            .expect("directory should be valid");
    }

    #[test]
    fn validate_overflow() {
        let mut d = Directory::new();

        assert_eq!(
            d.add(Node::Directory(
                DirectoryNode::new("foo".into(), DUMMY_DIGEST.clone(), u64::MAX).unwrap(),
            )),
            Err(ValidateDirectoryError::SizeOverflow)
        );
    }

    #[test]
    fn add_duplicate_node_to_directory() {
        let mut d = Directory::new();

        d.add(Node::Directory(
            DirectoryNode::new("a".into(), DUMMY_DIGEST.clone(), 1).unwrap(),
        ))
        .unwrap();
        assert_eq!(
            format!(
                "{}",
                d.add(Node::File(
                    FileNode::new("a".into(), DUMMY_DIGEST.clone(), 1, true).unwrap(),
                ))
                .expect_err("adding duplicate dir entry must fail")
            ),
            "\"a\" is a duplicate name"
        );
    }

    /// Attempt to add a directory entry with a name which should be rejected.
    #[tokio::test]
    async fn directory_reject_invalid_name() {
        let mut dir = Directory::new();
        assert!(
            dir.add(Node::Symlink(
                SymlinkNode::new(
                    "".into(), // wrong! can not be added to directory
                    "doesntmatter".into(),
                )
                .unwrap()
            ))
            .is_err(),
            "invalid symlink entry be rejected"
        );
    }
}
