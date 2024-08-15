use std::collections::BTreeMap;

use crate::{errors::DirectoryError, proto, B3Digest, Node};

/// A Directory contains nodes, which can be Directory, File or Symlink nodes.
/// It attached names to these nodes, which is the basename in that directory.
/// These names:
///  - MUST not contain slashes or null bytes
///  - MUST not be '.' or '..'
///  - MUST be unique across all three lists
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    nodes: BTreeMap<bytes::Bytes, Node>,
}

impl Directory {
    /// Constructs a new, empty Directory.
    /// FUTUREWORK: provide a constructor from an interator of (sorted) names and nodes.
    pub fn new() -> Self {
        Directory {
            nodes: BTreeMap::new(),
        }
    }

    /// The size of a directory is the number of all regular and symlink elements,
    /// the number of directory elements, and their size fields.
    pub fn size(&self) -> u64 {
        // It's impossible to create a Directory where the size overflows, because we
        // check before every add() that the size won't overflow.
        (self.nodes.len() as u64)
            + self
                .nodes()
                .map(|(_name, n)| match n {
                    Node::Directory { size, .. } => 1 + size,
                    Node::File { .. } | Node::Symlink { .. } => 1,
                })
                .sum::<u64>()
    }

    /// Calculates the digest of a Directory, which is the blake3 hash of a
    /// Directory protobuf message, serialized in protobuf canonical form.
    pub fn digest(&self) -> B3Digest {
        proto::Directory::from(self.clone()).digest()
    }

    /// Allows iterating over all nodes (directories, files and symlinks)
    /// For each, it returns a tuple of its name and node.
    /// The elements are sorted by their names.
    pub fn nodes(&self) -> impl Iterator<Item = (&bytes::Bytes, &Node)> + Send + Sync + '_ {
        self.nodes.iter()
    }

    /// Checks a Node name for validity as a directory entry
    /// We disallow slashes, null bytes, '.', '..' and the empty string.
    pub(crate) fn is_valid_name(name: &[u8]) -> bool {
        !(name.is_empty()
            || name == b".."
            || name == b"."
            || name.contains(&0x00)
            || name.contains(&b'/'))
    }

    /// Adds the specified [Node] to the [Directory] with a given name.
    ///
    /// Inserting an element that already exists with the same name in the directory will yield an
    /// error.
    /// Inserting an element will validate that its name fulfills the
    /// requirements for directory entries and yield an error if it is not.
    pub fn add(&mut self, name: bytes::Bytes, node: Node) -> Result<(), DirectoryError> {
        if !Self::is_valid_name(&name) {
            return Err(DirectoryError::InvalidName(name));
        }

        // Check that the even after adding this new directory entry, the size calculation will not
        // overflow
        // FUTUREWORK: add some sort of batch add interface which only does this check once with
        // all the to-be-added entries
        checked_sum([
            self.size(),
            1,
            match node {
                Node::Directory { size, .. } => size,
                _ => 0,
            },
        ])
        .ok_or(DirectoryError::SizeOverflow)?;

        match self.nodes.entry(name) {
            std::collections::btree_map::Entry::Vacant(e) => {
                e.insert(node);
                Ok(())
            }
            std::collections::btree_map::Entry::Occupied(occupied) => {
                Err(DirectoryError::DuplicateName(occupied.key().to_vec()))
            }
        }
    }
}

fn checked_sum(iter: impl IntoIterator<Item = u64>) -> Option<u64> {
    iter.into_iter().try_fold(0u64, |acc, i| acc.checked_add(i))
}

#[cfg(test)]
mod test {
    use super::{Directory, Node};
    use crate::fixtures::DUMMY_DIGEST;
    use crate::DirectoryError;

    #[test]
    fn add_nodes_to_directory() {
        let mut d = Directory::new();

        d.add(
            "b".into(),
            Node::Directory {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
            },
        )
        .unwrap();
        d.add(
            "a".into(),
            Node::Directory {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
            },
        )
        .unwrap();
        d.add(
            "z".into(),
            Node::Directory {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
            },
        )
        .unwrap();

        d.add(
            "f".into(),
            Node::File {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
                executable: true,
            },
        )
        .unwrap();
        d.add(
            "c".into(),
            Node::File {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
                executable: true,
            },
        )
        .unwrap();
        d.add(
            "g".into(),
            Node::File {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
                executable: true,
            },
        )
        .unwrap();

        d.add(
            "t".into(),
            Node::Symlink {
                target: "a".try_into().unwrap(),
            },
        )
        .unwrap();
        d.add(
            "o".into(),
            Node::Symlink {
                target: "a".try_into().unwrap(),
            },
        )
        .unwrap();
        d.add(
            "e".into(),
            Node::Symlink {
                target: "a".try_into().unwrap(),
            },
        )
        .unwrap();

        // Convert to proto struct and back to ensure we are not generating any invalid structures
        crate::Directory::try_from(crate::proto::Directory::from(d))
            .expect("directory should be valid");
    }

    #[test]
    fn validate_overflow() {
        let mut d = Directory::new();

        assert_eq!(
            d.add(
                "foo".into(),
                Node::Directory {
                    digest: DUMMY_DIGEST.clone(),
                    size: u64::MAX
                }
            ),
            Err(DirectoryError::SizeOverflow)
        );
    }

    #[test]
    fn add_duplicate_node_to_directory() {
        let mut d = Directory::new();

        d.add(
            "a".into(),
            Node::Directory {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
            },
        )
        .unwrap();
        assert_eq!(
            format!(
                "{}",
                d.add(
                    "a".into(),
                    Node::File {
                        digest: DUMMY_DIGEST.clone(),
                        size: 1,
                        executable: true
                    }
                )
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
            dir.add(
                "".into(), // wrong! can not be added to directory
                Node::Symlink {
                    target: "doesntmatter".try_into().unwrap(),
                },
            )
            .is_err(),
            "invalid symlink entry be rejected"
        );
    }
}
