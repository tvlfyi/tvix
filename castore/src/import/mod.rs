//! The main library function here is [ingest_entries], receiving a stream of
//! [IngestionEntry].
//!
//! Specific implementations, such as ingesting from the filesystem, live in
//! child modules.

use crate::directoryservice::DirectoryPutter;
use crate::directoryservice::DirectoryService;
use crate::path::PathBuf;
use crate::proto::node::Node;
use crate::proto::Directory;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::B3Digest;
use crate::Path;
use futures::{Stream, StreamExt};

use tracing::Level;

use std::collections::HashMap;
use tracing::instrument;

mod error;
pub use error::IngestionError;

pub mod archive;
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

        let name = entry
            .path()
            .file_name()
            // If this is the root node, it will have an empty name.
            .unwrap_or_default()
            .to_owned()
            .into();

        let node = match &mut entry {
            IngestionEntry::Dir { .. } => {
                // If the entry is a directory, we traversed all its children (and
                // populated it in `directories`).
                // If we don't have it in there, it's an empty directory.
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

                Node::Directory(DirectoryNode {
                    name,
                    digest: directory_digest.into(),
                    size: directory_size,
                })
            }
            IngestionEntry::Symlink { ref target, .. } => Node::Symlink(SymlinkNode {
                name,
                target: target.to_owned().into(),
            }),
            IngestionEntry::Regular {
                size,
                executable,
                digest,
                ..
            } => Node::File(FileNode {
                name,
                digest: digest.to_owned().into(),
                size: *size,
                executable: *executable,
            }),
        };

        let parent = entry
            .path()
            .parent()
            .expect("Tvix bug: got entry with root node");

        if parent == crate::Path::ROOT {
            break node;
        } else {
            // record node in parent directory, creating a new [Directory] if not there yet.
            directories.entry(parent.to_owned()).or_default().add(node);
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
            if let Node::Directory(directory_node) = &root_node {
                debug_assert_eq!(
                    root_directory_digest,
                    directory_node
                        .digest
                        .to_vec()
                        .try_into()
                        .expect("invalid digest len")
                )
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
