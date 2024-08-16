use std::collections::BTreeMap;

use crate::{errors::DirectoryError, path::PathComponent, proto, B3Digest, Node};

/// A Directory contains nodes, which can be Directory, File or Symlink nodes.
/// It attached names to these nodes, which is the basename in that directory.
/// These names:
///  - MUST not contain slashes or null bytes
///  - MUST not be '.' or '..'
///  - MUST be unique across all three lists
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct Directory {
    nodes: BTreeMap<PathComponent, Node>,
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
    pub fn nodes(&self) -> impl Iterator<Item = (&PathComponent, &Node)> + Send + Sync + '_ {
        self.nodes.iter()
    }

    /// Dissolves a Directory into its individual names and nodes.
    /// The elements are sorted by their names.
    pub fn into_nodes(self) -> impl Iterator<Item = (PathComponent, Node)> + Send + Sync {
        self.nodes.into_iter()
    }

    /// Adds the specified [Node] to the [Directory] with a given name.
    ///
    /// Inserting an element that already exists with the same name in the directory will yield an
    /// error.
    /// Inserting an element will validate that its name fulfills the
    /// requirements for directory entries and yield an error if it is not.
    pub fn add(&mut self, name: PathComponent, node: Node) -> Result<(), DirectoryError> {
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
                Err(DirectoryError::DuplicateName(occupied.key().to_owned()))
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
            "b".try_into().unwrap(),
            Node::Directory {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
            },
        )
        .unwrap();
        d.add(
            "a".try_into().unwrap(),
            Node::Directory {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
            },
        )
        .unwrap();
        d.add(
            "z".try_into().unwrap(),
            Node::Directory {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
            },
        )
        .unwrap();

        d.add(
            "f".try_into().unwrap(),
            Node::File {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
                executable: true,
            },
        )
        .unwrap();
        d.add(
            "c".try_into().unwrap(),
            Node::File {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
                executable: true,
            },
        )
        .unwrap();
        d.add(
            "g".try_into().unwrap(),
            Node::File {
                digest: DUMMY_DIGEST.clone(),
                size: 1,
                executable: true,
            },
        )
        .unwrap();

        d.add(
            "t".try_into().unwrap(),
            Node::Symlink {
                target: "a".try_into().unwrap(),
            },
        )
        .unwrap();
        d.add(
            "o".try_into().unwrap(),
            Node::Symlink {
                target: "a".try_into().unwrap(),
            },
        )
        .unwrap();
        d.add(
            "e".try_into().unwrap(),
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
                "foo".try_into().unwrap(),
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
            "a".try_into().unwrap(),
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
                    "a".try_into().unwrap(),
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
}
