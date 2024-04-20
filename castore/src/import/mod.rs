//! Deals with ingesting contents into castore.
//! The main library function here is [ingest_entries], receiving a stream of
//! [IngestionEntry].
//!
//! Specific implementations, such as ingesting from the filesystem, live in
//! child modules.

use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryPutter;
use crate::directoryservice::DirectoryService;
use crate::proto::node::Node;
use crate::proto::Directory;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::B3Digest;
use futures::Future;
use futures::{Stream, StreamExt};
use std::fs::FileType;
use std::pin::Pin;
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

pub mod fs;

/// Ingests [IngestionEntry] from the given stream into a the passed [DirectoryService].
/// On success, returns the root [Node].
///
/// The stream must have the following invariants:
/// - All children entries must come before their parents.
/// - The last entry must be the root node which must have a single path component.
/// - Every entry should have a unique path, and only consist of normal components.
///   This means, no windows path prefixes, absolute paths, `.` or `..`.
///
/// Internally we maintain a [HashMap] of [PathBuf] to partially populated [Directory] at that
/// path. Once we receive an [IngestionEntry] for the directory itself, we remove it from the
/// map and upload it to the [DirectoryService] through a lazily created [DirectoryPutter].
///
/// On success, returns the root node.
#[instrument(skip_all, ret(level = Level::TRACE), err)]
pub async fn ingest_entries<'a, DS, S>(directory_service: DS, mut entries: S) -> Result<Node, Error>
where
    DS: AsRef<dyn DirectoryService>,
    S: Stream<Item = Result<IngestionEntry<'a>, Error>> + Send + std::marker::Unpin,
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
                target: target.as_os_str().as_bytes().to_owned().into(),
            }),
            IngestionEntry::Regular {
                size,
                executable,
                digest,
                ..
            } => Node::File(FileNode {
                name,
                digest: digest.await?.into(),
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

    // if there were directories uploaded, make sure we flush the putter, so
    // they're all persisted to the backend.
    if let Some(mut directory_putter) = maybe_directory_putter {
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

/// Uploads the file at the provided [Path] the the [BlobService].
#[instrument(skip(blob_service), fields(path), err)]
async fn upload_blob_at_path<BS>(blob_service: BS, path: PathBuf) -> Result<B3Digest, Error>
where
    BS: BlobService,
{
    let mut file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(e) => return Err(Error::UnableToRead(path, e)),
    };

    let mut writer = blob_service.open_write().await;

    if let Err(e) = tokio::io::copy(&mut file, &mut writer).await {
        return Err(Error::UnableToRead(path, e));
    };

    let digest = writer
        .close()
        .await
        .map_err(|e| Error::UnableToRead(path, e))?;

    Ok(digest)
}

type BlobFut<'a> = Pin<Box<dyn Future<Output = Result<B3Digest, Error>> + Send + 'a>>;

pub enum IngestionEntry<'a> {
    Regular {
        path: PathBuf,
        size: u64,
        executable: bool,
        digest: BlobFut<'a>,
    },
    Symlink {
        path: PathBuf,
        target: PathBuf,
    },
    Dir {
        path: PathBuf,
    },
    Unknown {
        path: PathBuf,
        file_type: FileType,
    },
}

impl<'a> IngestionEntry<'a> {
    fn path(&self) -> &Path {
        match self {
            IngestionEntry::Regular { path, .. } => path,
            IngestionEntry::Symlink { path, .. } => path,
            IngestionEntry::Dir { path } => path,
            IngestionEntry::Unknown { path, .. } => path,
        }
    }
}
