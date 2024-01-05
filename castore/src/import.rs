use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryPutter;
use crate::directoryservice::DirectoryService;
use crate::proto::node::Node;
use crate::proto::Directory;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::Error as CastoreError;
use std::os::unix::ffi::OsStrExt;
use std::{
    collections::HashMap,
    fmt::Debug,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};
use tracing::instrument;
use walkdir::DirEntry;
use walkdir::WalkDir;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to upload directory at {0}: {1}")]
    UploadDirectoryError(PathBuf, CastoreError),

    #[error("invalid encoding encountered for entry {0:?}")]
    InvalidEncoding(PathBuf),

    #[error("unable to stat {0}: {1}")]
    UnableToStat(PathBuf, std::io::Error),

    #[error("unable to open {0}: {1}")]
    UnableToOpen(PathBuf, std::io::Error),

    #[error("unable to read {0}: {1}")]
    UnableToRead(PathBuf, std::io::Error),
}

impl From<CastoreError> for Error {
    fn from(value: CastoreError) -> Self {
        match value {
            CastoreError::InvalidRequest(_) => panic!("tvix bug"),
            CastoreError::StorageError(_) => panic!("error"),
        }
    }
}

impl From<Error> for std::io::Error {
    fn from(value: Error) -> Self {
        std::io::Error::new(std::io::ErrorKind::Other, value)
    }
}

/// This processes a given [walkdir::DirEntry] and returns a
/// proto::node::Node, depending on the type of the entry.
///
/// If the entry is a file, its contents are uploaded.
/// If the entry is a directory, the Directory is uploaded as well.
/// For this to work, it relies on the caller to provide the directory object
/// with the previously returned (child) nodes.
///
/// It assumes to be called only if all children of it have already been processed. If the entry is
/// indeed a directory, it'll also upload that directory to the store. For this, the
/// so-far-assembled Directory object for this path needs to be passed in.
///
/// It assumes the caller adds returned nodes to the directories it assembles.
#[instrument(skip_all, fields(entry.file_type=?&entry.file_type(),entry.path=?entry.path()))]
async fn process_entry<'a, BS>(
    blob_service: BS,
    directory_putter: &'a mut Box<dyn DirectoryPutter>,
    entry: &'a walkdir::DirEntry,
    maybe_directory: Option<Directory>,
) -> Result<Node, Error>
where
    BS: AsRef<dyn BlobService> + Clone,
{
    let file_type = entry.file_type();

    if file_type.is_dir() {
        let directory = maybe_directory
            .expect("tvix bug: must be called with some directory in the case of directory");
        let directory_digest = directory.digest();
        let directory_size = directory.size();

        // upload this directory
        directory_putter
            .put(directory)
            .await
            .map_err(|e| Error::UploadDirectoryError(entry.path().to_path_buf(), e))?;

        return Ok(Node::Directory(DirectoryNode {
            name: entry.file_name().as_bytes().to_owned().into(),
            digest: directory_digest.into(),
            size: directory_size,
        }));
    }

    if file_type.is_symlink() {
        let target: bytes::Bytes = std::fs::read_link(entry.path())
            .map_err(|e| Error::UnableToStat(entry.path().to_path_buf(), e))?
            .as_os_str()
            .as_bytes()
            .to_owned()
            .into();

        return Ok(Node::Symlink(SymlinkNode {
            name: entry.file_name().as_bytes().to_owned().into(),
            target,
        }));
    }

    if file_type.is_file() {
        let metadata = entry
            .metadata()
            .map_err(|e| Error::UnableToStat(entry.path().to_path_buf(), e.into()))?;

        let mut file = tokio::fs::File::open(entry.path())
            .await
            .map_err(|e| Error::UnableToOpen(entry.path().to_path_buf(), e))?;

        let mut writer = blob_service.as_ref().open_write().await;

        if let Err(e) = tokio::io::copy(&mut file, &mut writer).await {
            return Err(Error::UnableToRead(entry.path().to_path_buf(), e));
        };

        let digest = writer
            .close()
            .await
            .map_err(|e| Error::UnableToRead(entry.path().to_path_buf(), e))?;

        return Ok(Node::File(FileNode {
            name: entry.file_name().as_bytes().to_vec().into(),
            digest: digest.into(),
            size: metadata.len(),
            // If it's executable by the user, it'll become executable.
            // This matches nix's dump() function behaviour.
            executable: metadata.permissions().mode() & 64 != 0,
        }));
    }
    todo!("handle other types")
}

/// Ingests the contents at the given path into the tvix store,
/// interacting with a [BlobService] and [DirectoryService].
/// It returns the root node or an error.
///
/// It does not follow symlinks at the root, they will be ingested as actual
/// symlinks.
///
/// It's not interacting with a PathInfoService (from tvix-store), or anything
/// else giving it a "non-content-addressed name".
/// It's up to the caller to possibly register it somewhere (and potentially
/// rename it based on some naming scheme)
#[instrument(skip(blob_service, directory_service), fields(path=?p), err)]
pub async fn ingest_path<'a, BS, DS, P>(
    blob_service: BS,
    directory_service: DS,
    p: P,
) -> Result<Node, Error>
where
    P: AsRef<Path> + Debug,
    BS: AsRef<dyn BlobService> + Clone,
    DS: AsRef<dyn DirectoryService>,
{
    let mut directories: HashMap<PathBuf, Directory> = HashMap::default();

    let mut directory_putter = directory_service.as_ref().put_multiple_start();

    let mut entries_per_depths: Vec<Vec<DirEntry>> = vec![Vec::new()];
    for entry in WalkDir::new(p.as_ref())
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(false)
        .sort_by_file_name()
        .into_iter()
    {
        // Entry could be a NotFound, if the root path specified does not exist.
        let entry = entry.map_err(|e| {
            Error::UnableToOpen(
                PathBuf::from(p.as_ref()),
                e.into_io_error().expect("walkdir err must be some"),
            )
        })?;

        if entry.depth() >= entries_per_depths.len() {
            debug_assert!(
                entry.depth() == entries_per_depths.len(),
                "Received unexpected entry with depth {} during descent, previously at {}",
                entry.depth(),
                entries_per_depths.len()
            );

            entries_per_depths.push(vec![entry]);
        } else {
            entries_per_depths[entry.depth()].push(entry);
        }
    }

    debug_assert!(!entries_per_depths[0].is_empty(), "No root node available!");

    // We need to process a directory's children before processing
    // the directory itself in order to have all the data needed
    // to compute the hash.
    for level in entries_per_depths.into_iter().rev() {
        for entry in level.into_iter() {
            // FUTUREWORK: inline `process_entry`
            let node = process_entry(
                blob_service.clone(),
                &mut directory_putter,
                &entry,
                // process_entry wants an Option<Directory> in case the entry points to a directory.
                // make sure to provide it.
                // If the directory has contents, we already have it in
                // `directories` because we iterate over depth in reverse order (deepest to
                // shallowest).
                if entry.file_type().is_dir() {
                    Some(
                        directories
                            .remove(entry.path())
                            // In that case, it contained no children
                            .unwrap_or_default(),
                    )
                } else {
                    None
                },
            )
            .await?;

            if entry.depth() == 0 {
                // Make sure all the directories are flushed.
                // FUTUREWORK: `debug_assert!` the resulting Ok(b3_digest) to be equal
                // to `directories.get(entry.path())`.
                if entry.file_type().is_dir() {
                    directory_putter.close().await?;
                }
                return Ok(node);
            } else {
                // calculate the parent path, and make sure we register the node there.
                // NOTE: entry.depth() > 0
                let parent_path = entry.path().parent().unwrap().to_path_buf();

                // record node in parent directory, creating a new [proto:Directory] if not there yet.
                let parent_directory = directories.entry(parent_path).or_default();
                match node {
                    Node::Directory(e) => parent_directory.directories.push(e),
                    Node::File(e) => parent_directory.files.push(e),
                    Node::Symlink(e) => parent_directory.symlinks.push(e),
                }
            }
        }
    }
    // unreachable, we already bailed out before if root doesn't exist.
    unreachable!()
}
