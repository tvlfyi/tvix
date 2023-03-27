use crate::{chunkservice::read_all_and_chunk, directoryservice::DirectoryPutter, proto};
use std::{
    collections::HashMap,
    fmt::Debug,
    fs,
    fs::File,
    os::unix::prelude::PermissionsExt,
    path::{Path, PathBuf},
};
use tracing::instrument;
use walkdir::WalkDir;

use crate::{
    blobservice::BlobService, chunkservice::ChunkService, directoryservice::DirectoryService,
};

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
fn process_entry<BS: BlobService, CS: ChunkService + std::marker::Sync, DP: DirectoryPutter>(
    blob_service: &mut BS,
    chunk_service: &mut CS,
    directory_putter: &mut DP,
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
            name: entry
                .file_name()
                .to_str()
                .map(|s| Ok(s.to_owned()))
                .unwrap_or(Err(Error::InvalidEncoding(entry.path().to_path_buf())))?,
            digest: directory_digest.to_vec(),
            size: directory_size,
        }));
    }

    if file_type.is_symlink() {
        let target = std::fs::read_link(&entry_path)
            .map_err(|e| Error::UnableToStat(entry_path.clone(), e))?;

        return Ok(proto::node::Node::Symlink(proto::SymlinkNode {
            name: entry
                .file_name()
                .to_str()
                .map(|s| Ok(s.to_owned()))
                .unwrap_or(Err(Error::InvalidEncoding(entry.path().to_path_buf())))?,
            target: target
                .to_str()
                .map(|s| Ok(s.to_owned()))
                .unwrap_or(Err(Error::InvalidEncoding(entry.path().to_path_buf())))?,
        }));
    }

    if file_type.is_file() {
        let metadata = entry
            .metadata()
            .map_err(|e| Error::UnableToStat(entry_path.clone(), e.into()))?;

        let file = File::open(entry_path.clone())
            .map_err(|e| Error::UnableToOpen(entry_path.clone(), e))?;

        let (blob_digest, blob_meta) = read_all_and_chunk(chunk_service, file)?;

        // upload blobmeta if not there yet
        if blob_service
            .stat(&proto::StatBlobRequest {
                digest: blob_digest.to_vec(),
                include_chunks: false,
                include_bao: false,
            })?
            .is_none()
        {
            // upload blobmeta
            blob_service.put(&blob_digest, blob_meta)?;
        }

        return Ok(proto::node::Node::File(proto::FileNode {
            name: entry
                .file_name()
                .to_str()
                .map(|s| Ok(s.to_owned()))
                .unwrap_or(Err(Error::InvalidEncoding(entry.path().to_path_buf())))?,
            digest: blob_digest,
            size: metadata.len() as u32,
            // If it's executable by the user, it'll become executable.
            // This matches nix's dump() function behaviour.
            executable: metadata.permissions().mode() & 64 != 0,
        }));
    }
    todo!("handle other types")
}

/// Imports the contents at a given Path into the tvix store.
///
/// It doesn't register the contents at a Path in the store itself, that's up
/// to the PathInfoService.
//
// returns the root node, or an error.
#[instrument(skip(blob_service, chunk_service, directory_service), fields(path=?p))]
pub fn import_path<
    BS: BlobService,
    CS: ChunkService + std::marker::Sync,
    DS: DirectoryService,
    P: AsRef<Path> + Debug,
>(
    blob_service: &mut BS,
    chunk_service: &mut CS,
    directory_service: &mut DS,
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
                .to_str()
                .map(|s| Ok(s.to_owned()))
                .unwrap_or(Err(Error::InvalidEncoding(p.as_ref().to_path_buf())))?,
            target: target
                .to_str()
                .map(|s| Ok(s.to_owned()))
                .unwrap_or(Err(Error::InvalidEncoding(p.as_ref().to_path_buf())))?,
        }));
    }

    let mut directories: HashMap<PathBuf, proto::Directory> = HashMap::default();

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
            blob_service,
            chunk_service,
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
