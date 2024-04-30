//! The main library function here is [ingest_entries], receiving a stream of
//! [IngestionEntry].
//!
//! Specific implementations, such as ingesting from the filesystem, live in
//! child modules.

use crate::directoryservice::DirectoryPutter;
use crate::directoryservice::DirectoryService;
use crate::proto::node::Node;
use crate::proto::Directory;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::B3Digest;
use futures::{Stream, StreamExt};
use std::fs::FileType;

use tracing::Level;

#[cfg(target_family = "unix")]
use std::os::unix::ffi::OsStrExt;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tracing::instrument;

mod error;
pub use error::Error;

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
pub async fn ingest_entries<DS, S>(directory_service: DS, mut entries: S) -> Result<Node, Error>
where
    DS: AsRef<dyn DirectoryService>,
    S: Stream<Item = Result<IngestionEntry, Error>> + Send + std::marker::Unpin,
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

        debug_assert!(
            entry
                .path()
                .components()
                .all(|x| matches!(x, std::path::Component::Normal(_))),
            "path may only contain normal components"
        );

        let name = entry
            .path()
            .file_name()
            // If this is the root node, it will have an empty name.
            .unwrap_or_default()
            .as_bytes()
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
                    .get_or_insert_with(|| directory_service.as_ref().put_multiple_start())
                    .put(directory)
                    .await?;

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
            IngestionEntry::Unknown { path, file_type } => {
                return Err(Error::UnsupportedFileType(path.clone(), *file_type));
            }
        };

        if entry.path().components().count() == 1 {
            break node;
        }

        // record node in parent directory, creating a new [Directory] if not there yet.
        directories
            .entry(entry.path().parent().unwrap().to_path_buf())
            .or_default()
            .add(node);
    };

    assert!(
        directories.is_empty(),
        "Tvix bug: left over directories after processing ingestion stream"
    );

    // if there were directories uploaded, make sure we flush the putter, so
    // they're all persisted to the backend.
    if let Some(mut directory_putter) = maybe_directory_putter {
        #[cfg_attr(not(debug_assertions), allow(unused))]
        let root_directory_digest = directory_putter.close().await?;

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
    Unknown {
        path: PathBuf,
        file_type: FileType,
    },
}

impl IngestionEntry {
    fn path(&self) -> &Path {
        match self {
            IngestionEntry::Regular { path, .. } => path,
            IngestionEntry::Symlink { path, .. } => path,
            IngestionEntry::Dir { path } => path,
            IngestionEntry::Unknown { path, .. } => path,
        }
    }

    fn is_dir(&self) -> bool {
        matches!(self, IngestionEntry::Dir { .. })
    }
}
