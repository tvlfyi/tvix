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
use std::sync::Arc;
use std::{
    collections::HashMap,
    fmt::Debug,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};
use tracing::instrument;
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

// This processes a given [walkdir::DirEntry] and returns a
// proto::node::Node, depending on the type of the entry.
//
// If the entry is a file, its contents are uploaded.
// If the entry is a directory, the Directory is uploaded as well.
// For this to work, it relies on the caller to provide the directory object
// with the previously returned (child) nodes.
//
// It assumes entries to be returned in "contents first" order, means this
// will only be called with a directory if all children of it have been
// visited. If the entry is indeed a directory, it'll also upload that
// directory to the store. For this, the so-far-assembled Directory object for
// this path needs to be passed in.
//
// It assumes the caller adds returned nodes to the directories it assembles.
#[instrument(skip_all, fields(entry.file_type=?&entry.file_type(),entry.path=?entry.path()))]
async fn process_entry<'a>(
    blob_service: Arc<dyn BlobService>,
    directory_putter: &'a mut Box<dyn DirectoryPutter>,
    entry: &'a walkdir::DirEntry,
    maybe_directory: Option<Directory>,
) -> Result<Node, Error> {
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

        let mut writer = blob_service.open_write().await;

        if let Err(e) = tokio::io::copy(&mut file, &mut writer).await {
            return Err(Error::UnableToRead(entry.path().to_path_buf(), e));
        };

        let digest = writer.close().await?;

        return Ok(Node::File(FileNode {
            name: entry.file_name().as_bytes().to_vec().into(),
            digest: digest.into(),
            size: metadata.len() as u32,
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
#[instrument(skip(blob_service, directory_service), fields(path=?p))]
pub async fn ingest_path<P: AsRef<Path> + Debug>(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    p: P,
) -> Result<Node, Error> {
    let mut directories: HashMap<PathBuf, Directory> = HashMap::default();

    let mut directory_putter = directory_service.put_multiple_start();

    for entry in WalkDir::new(p)
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(true)
        .sort_by_file_name()
    {
        let entry = entry.unwrap();

        // process_entry wants an Option<Directory> in case the entry points to a directory.
        // make sure to provide it.
        let maybe_directory: Option<Directory> = {
            if entry.file_type().is_dir() {
                Some(
                    directories
                        .entry(entry.path().to_path_buf())
                        .or_default()
                        .clone(),
                )
            } else {
                None
            }
        };

        let node = process_entry(
            blob_service.clone(),
            &mut directory_putter,
            &entry,
            maybe_directory,
        )
        .await?;

        if entry.depth() == 0 {
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
    // unreachable, we already bailed out before if root doesn't exist.
    panic!("tvix bug")
}
