use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryService;
use crate::{directoryservice::DirectoryPutter, proto};
use std::os::unix::ffi::OsStrExt;
use std::sync::Arc;
use std::{
    collections::HashMap,
    fmt::Debug,
    fs,
    fs::File,
    io,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};
use tracing::instrument;
use walkdir::WalkDir;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("failed to upload directory at {0}: {1}")]
    UploadDirectoryError(PathBuf, crate::Error),

    #[error("invalid encoding encountered for entry {0:?}")]
    InvalidEncoding(PathBuf),

    #[error("unable to stat {0}: {1}")]
    UnableToStat(PathBuf, std::io::Error),

    #[error("unable to open {0}: {1}")]
    UnableToOpen(PathBuf, std::io::Error),

    #[error("unable to read {0}: {1}")]
    UnableToRead(PathBuf, std::io::Error),
}

impl From<super::Error> for Error {
    fn from(value: super::Error) -> Self {
        match value {
            crate::Error::InvalidRequest(_) => panic!("tvix bug"),
            crate::Error::StorageError(_) => panic!("error"),
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
fn process_entry(
    blob_service: Arc<dyn BlobService>,
    directory_putter: &mut Box<dyn DirectoryPutter>,
    entry: &walkdir::DirEntry,
    maybe_directory: Option<proto::Directory>,
) -> Result<proto::node::Node, Error> {
    let file_type = entry.file_type();

    let entry_path: PathBuf = entry.path().to_path_buf();

    if file_type.is_dir() {
        let directory = maybe_directory
            .expect("tvix bug: must be called with some directory in the case of directory");
        let directory_digest = directory.digest();
        let directory_size = directory.size();

        // upload this directory
        directory_putter
            .put(directory)
            .map_err(|e| Error::UploadDirectoryError(entry.path().to_path_buf(), e))?;

        return Ok(proto::node::Node::Directory(proto::DirectoryNode {
            name: entry.file_name().as_bytes().to_vec(),
            digest: directory_digest.to_vec(),
            size: directory_size,
        }));
    }

    if file_type.is_symlink() {
        let target = std::fs::read_link(&entry_path)
            .map_err(|e| Error::UnableToStat(entry_path.clone(), e))?;

        return Ok(proto::node::Node::Symlink(proto::SymlinkNode {
            name: entry.file_name().as_bytes().to_vec(),
            target: target.as_os_str().as_bytes().to_vec(),
        }));
    }

    if file_type.is_file() {
        let metadata = entry
            .metadata()
            .map_err(|e| Error::UnableToStat(entry_path.clone(), e.into()))?;

        let mut file = File::open(entry_path.clone())
            .map_err(|e| Error::UnableToOpen(entry_path.clone(), e))?;

        let mut writer = blob_service.open_write();

        if let Err(e) = io::copy(&mut file, &mut writer) {
            return Err(Error::UnableToRead(entry_path, e));
        };

        let digest = writer.close()?;

        return Ok(proto::node::Node::File(proto::FileNode {
            name: entry.file_name().as_bytes().to_vec(),
            digest: digest.to_vec(),
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
/// It's not interacting with a [PathInfoService], it's up to the caller to
/// possibly register it somewhere (and potentially rename it based on some
/// naming scheme.
#[instrument(skip(blob_service, directory_service), fields(path=?p))]
pub fn ingest_path<P: AsRef<Path> + Debug>(
    blob_service: Arc<dyn BlobService>,
    directory_service: Arc<dyn DirectoryService>,
    p: P,
) -> Result<proto::node::Node, Error> {
    // Probe if the path points to a symlink. If it does, we process it manually,
    // due to https://github.com/BurntSushi/walkdir/issues/175.
    let symlink_metadata = fs::symlink_metadata(p.as_ref())
        .map_err(|e| Error::UnableToStat(p.as_ref().to_path_buf(), e))?;
    if symlink_metadata.is_symlink() {
        let target = std::fs::read_link(p.as_ref())
            .map_err(|e| Error::UnableToStat(p.as_ref().to_path_buf(), e))?;
        return Ok(proto::node::Node::Symlink(proto::SymlinkNode {
            name: p
                .as_ref()
                .file_name()
                .unwrap_or_default()
                .as_bytes()
                .to_vec(),
            target: target.as_os_str().as_bytes().to_vec(),
        }));
    }

    let mut directories: HashMap<PathBuf, proto::Directory> = HashMap::default();

    // TODO: pass this one instead?
    let mut directory_putter = directory_service.put_multiple_start();

    for entry in WalkDir::new(p)
        .follow_links(false)
        .contents_first(true)
        .sort_by_file_name()
    {
        let entry = entry.unwrap();

        // process_entry wants an Option<Directory> in case the entry points to a directory.
        // make sure to provide it.
        let maybe_directory: Option<proto::Directory> = {
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
        )?;

        if entry.depth() == 0 {
            return Ok(node);
        } else {
            // calculate the parent path, and make sure we register the node there.
            // NOTE: entry.depth() > 0
            let parent_path = entry.path().parent().unwrap().to_path_buf();

            // record node in parent directory, creating a new [proto:Directory] if not there yet.
            let parent_directory = directories.entry(parent_path).or_default();
            match node {
                proto::node::Node::Directory(e) => parent_directory.directories.push(e),
                proto::node::Node::File(e) => parent_directory.files.push(e),
                proto::node::Node::Symlink(e) => parent_directory.symlinks.push(e),
            }
        }
    }
    // unreachable, we already bailed out before if root doesn't exist.
    panic!("tvix bug")
}
