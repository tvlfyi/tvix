//! The main library function here is [ingest_entries], receiving a stream of
//! [IngestionEntry].
//!
//! Specific implementations, such as ingesting from the filesystem, live in
//! child modules.

use crate::directoryservice::{DirectoryPutter, DirectoryService};
use crate::path::{Path, PathBuf};
use crate::{B3Digest, Directory, Node};
use futures::{Stream, StreamExt};
use tracing::Level;

use std::collections::HashMap;
use tracing::instrument;

mod error;
pub use error::IngestionError;

pub mod archive;
pub mod blobs;
pub mod fs;

/// Ingests [IngestionEntry] from the given stream into a the passed [DirectoryService].
/// On success, returns the root [Node].
///
/// The stream must have the following invariants:
/// - All children entries must come before their parents.
/// - The last entry must be the root node which must have a single path component.
/// - Every entry should have a unique path, and only consist of normal components.
///   This means, no windows path prefixes, absolute paths, `.` or `..`.
/// - All referenced directories must have an associated directory entry in the stream.
///   This means if there is a file entry for `foo/bar`, there must also be a `foo` directory
///   entry.
///
/// Internally we maintain a [HashMap] of [PathBuf] to partially populated [Directory] at that
/// path. Once we receive an [IngestionEntry] for the directory itself, we remove it from the
/// map and upload it to the [DirectoryService] through a lazily created [DirectoryPutter].
///
/// On success, returns the root node.
#[instrument(skip_all, ret(level = Level::TRACE), err)]
pub async fn ingest_entries<DS, S, E>(
    directory_service: DS,
    mut entries: S,
) -> Result<Node, IngestionError<E>>
where
    DS: DirectoryService,
    S: Stream<Item = Result<IngestionEntry, E>> + Send + std::marker::Unpin,
    E: std::error::Error,
{
    // For a given path, this holds the [Directory] structs as they are populated.
    let mut directories: HashMap<PathBuf, Directory> = HashMap::default();
    let mut maybe_directory_putter: Option<Box<dyn DirectoryPutter>> = None;

    let root_node = loop {
        let mut entry = entries
            .next()
            .await
            // The last entry of the stream must have 1 path component, after which
            // we break the loop manually.
            .expect("Tvix bug: unexpected end of stream")?;

        let node = match &mut entry {
            IngestionEntry::Dir { .. } => {
                // If the entry is a directory, we traversed all its children (and
                // populated it in `directories`).
                // If we don't have it in directories, it's a directory without
                // children.
                let directory = directories
                    .remove(entry.path())
                    // In that case, it contained no children
                    .unwrap_or_default();

                let directory_size = directory.size();
                let directory_digest = directory.digest();

                // Use the directory_putter to upload the directory.
                // If we don't have one yet (as that's the first one to upload),
                // initialize the putter.
                maybe_directory_putter
                    .get_or_insert_with(|| directory_service.put_multiple_start())
                    .put(directory)
                    .await
                    .map_err(|e| {
                        IngestionError::UploadDirectoryError(entry.path().to_owned(), e)
                    })?;

                Node::Directory {
                    digest: directory_digest,
                    size: directory_size,
                }
            }
            IngestionEntry::Symlink { ref target, .. } => Node::Symlink {
                target: bytes::Bytes::copy_from_slice(target).try_into().map_err(
                    |e: crate::ValidateNodeError| {
                        IngestionError::UploadDirectoryError(
                            entry.path().to_owned(),
                            crate::Error::StorageError(e.to_string()),
                        )
                    },
                )?,
            },
            IngestionEntry::Regular {
                size,
                executable,
                digest,
                ..
            } => Node::File {
                digest: digest.clone(),
                size: *size,
                executable: *executable,
            },
        };

        let parent = entry
            .path()
            .parent()
            .expect("Tvix bug: got entry with root node");

        if parent == crate::Path::ROOT {
            break node;
        } else {
            let name = entry
                .path()
                .file_name()
                // If this is the root node, it will have an empty name.
                .unwrap_or_default()
                .to_owned()
                .into();

            // record node in parent directory, creating a new [Directory] if not there yet.
            directories
                .entry(parent.to_owned())
                .or_default()
                .add(name, node)
                .map_err(|e| {
                    IngestionError::UploadDirectoryError(
                        entry.path().to_owned(),
                        crate::Error::StorageError(e.to_string()),
                    )
                })?;
        }
    };

    assert!(
        entries.count().await == 0,
        "Tvix bug: left over elements in the stream"
    );

    assert!(
        directories.is_empty(),
        "Tvix bug: left over directories after processing ingestion stream"
    );

    // if there were directories uploaded, make sure we flush the putter, so
    // they're all persisted to the backend.
    if let Some(mut directory_putter) = maybe_directory_putter {
        #[cfg_attr(not(debug_assertions), allow(unused))]
        let root_directory_digest = directory_putter
            .close()
            .await
            .map_err(|e| IngestionError::FinalizeDirectoryUpload(e))?;

        #[cfg(debug_assertions)]
        {
            if let Node::Directory { digest, .. } = &root_node {
                debug_assert_eq!(&root_directory_digest, digest);
            } else {
                unreachable!("Tvix bug: directory putter initialized but no root directory node");
            }
        }
    };

    Ok(root_node)
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum IngestionEntry {
    Regular {
        path: PathBuf,
        size: u64,
        executable: bool,
        digest: B3Digest,
    },
    Symlink {
        path: PathBuf,
        target: Vec<u8>,
    },
    Dir {
        path: PathBuf,
    },
}

impl IngestionEntry {
    fn path(&self) -> &Path {
        match self {
            IngestionEntry::Regular { path, .. } => path,
            IngestionEntry::Symlink { path, .. } => path,
            IngestionEntry::Dir { path } => path,
        }
    }

    fn is_dir(&self) -> bool {
        matches!(self, IngestionEntry::Dir { .. })
    }
}

#[cfg(test)]
mod test {
    use rstest::rstest;

    use crate::fixtures::{DIRECTORY_COMPLICATED, DIRECTORY_WITH_KEEP, EMPTY_BLOB_DIGEST};
    use crate::{directoryservice::MemoryDirectoryService, fixtures::DUMMY_DIGEST};
    use crate::{Directory, Node};

    use super::ingest_entries;
    use super::IngestionEntry;

    #[rstest]
    #[case::single_file(vec![IngestionEntry::Regular {
        path: "foo".parse().unwrap(),
        size: 42,
        executable: true,
        digest: DUMMY_DIGEST.clone(),
    }],
        Node::File{digest: DUMMY_DIGEST.clone(), size: 42, executable: true}
    )]
    #[case::single_symlink(vec![IngestionEntry::Symlink {
        path: "foo".parse().unwrap(),
        target: b"blub".into(),
    }],
        Node::Symlink{target: "blub".try_into().unwrap()}
    )]
    #[case::single_dir(vec![IngestionEntry::Dir {
        path: "foo".parse().unwrap(),
    }],
        Node::Directory{digest: Directory::default().digest(), size: Directory::default().size()}
    )]
    #[case::dir_with_keep(vec![
        IngestionEntry::Regular {
            path: "foo/.keep".parse().unwrap(),
            size: 0,
            executable: false,
            digest: EMPTY_BLOB_DIGEST.clone(),
        },
        IngestionEntry::Dir {
            path: "foo".parse().unwrap(),
        },
    ],
        Node::Directory{ digest: DIRECTORY_WITH_KEEP.digest(), size: DIRECTORY_WITH_KEEP.size()}
    )]
    /// This is intentionally a bit unsorted, though it still satisfies all
    /// requirements we have on the order of elements in the stream.
    #[case::directory_complicated(vec![
        IngestionEntry::Regular {
            path: "blub/.keep".parse().unwrap(),
            size: 0,
            executable: false,
            digest: EMPTY_BLOB_DIGEST.clone(),
        },
        IngestionEntry::Regular {
            path: "blub/keep/.keep".parse().unwrap(),
            size: 0,
            executable: false,
            digest: EMPTY_BLOB_DIGEST.clone(),
        },
        IngestionEntry::Dir {
            path: "blub/keep".parse().unwrap(),
        },
        IngestionEntry::Symlink {
            path: "blub/aa".parse().unwrap(),
            target: b"/nix/store/somewhereelse".into(),
        },
        IngestionEntry::Dir {
            path: "blub".parse().unwrap(),
        },
    ],
    Node::Directory{ digest: DIRECTORY_COMPLICATED.digest(), size: DIRECTORY_COMPLICATED.size() }
    )]
    #[tokio::test]
    async fn test_ingestion(#[case] entries: Vec<IngestionEntry>, #[case] exp_root_node: Node) {
        let directory_service = MemoryDirectoryService::default();

        let root_node = ingest_entries(
            directory_service.clone(),
            futures::stream::iter(entries.into_iter().map(Ok::<_, std::io::Error>)),
        )
        .await
        .expect("must succeed");

        assert_eq!(exp_root_node, root_node, "root node should match");
    }

    #[rstest]
    #[should_panic]
    #[case::empty_entries(vec![])]
    #[should_panic]
    #[case::missing_intermediate_dir(vec![
        IngestionEntry::Regular {
            path: "blub/.keep".parse().unwrap(),
            size: 0,
            executable: false,
            digest: EMPTY_BLOB_DIGEST.clone(),
        },
    ])]
    #[should_panic]
    #[case::leaf_after_parent(vec![
        IngestionEntry::Dir {
            path: "blub".parse().unwrap(),
        },
        IngestionEntry::Regular {
            path: "blub/.keep".parse().unwrap(),
            size: 0,
            executable: false,
            digest: EMPTY_BLOB_DIGEST.clone(),
        },
    ])]
    #[should_panic]
    #[case::root_in_entry(vec![
        IngestionEntry::Regular {
            path: ".keep".parse().unwrap(),
            size: 0,
            executable: false,
            digest: EMPTY_BLOB_DIGEST.clone(),
        },
        IngestionEntry::Dir {
            path: "".parse().unwrap(),
        },
    ])]
    #[tokio::test]
    async fn test_ingestion_fail(#[case] entries: Vec<IngestionEntry>) {
        let directory_service = MemoryDirectoryService::default();

        let _ = ingest_entries(
            directory_service.clone(),
            futures::stream::iter(entries.into_iter().map(Ok::<_, std::io::Error>)),
        )
        .await;
    }
}
