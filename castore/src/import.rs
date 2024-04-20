use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryPutter;
use crate::directoryservice::DirectoryService;
use crate::proto::node::Node;
use crate::proto::Directory;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;
use crate::B3Digest;
use crate::Error as CastoreError;
use futures::stream::BoxStream;
use futures::Future;
use futures::{Stream, StreamExt};
use std::fs::FileType;
use std::os::unix::fs::MetadataExt;
use std::pin::Pin;
use tracing::Level;

#[cfg(target_family = "unix")]
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

    #[error("unsupported file {0} type: {1:?}")]
    UnsupportedFileType(PathBuf, FileType),
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

/// Walk the filesystem at a given path and returns a stream of ingestion entries.
///
/// This is how [`ingest_path`] assembles the set of entries to pass on [`ingest_entries`].
/// This low-level function can be used if additional filtering or processing is required on the
/// entries.
///
/// It does not follow symlinks at the root, they will be ingested as actual symlinks.
///
/// This function will walk the filesystem using `walkdir` and will consume
/// `O(#number of entries)` space.
#[instrument(fields(path), skip(blob_service))]
fn walk_path_for_ingestion<'a, BS>(
    blob_service: BS,
    path: &'a Path,
) -> BoxStream<'a, Result<IngestionEntry<'a>, Error>>
where
    BS: BlobService + Clone + 'a,
{
    let iter = WalkDir::new(path)
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(true)
        .into_iter();

    dir_entry_iter_to_ingestion_stream(blob_service, iter, path)
}

/// Converts an iterator of [walkdir::DirEntry]s into a stream of ingestion entries.
/// This can then be fed into [ingest_entries] to ingest all the entries into the castore.
///
/// The root is the [Path] in the filesystem that is being ingested into the castore.
pub fn dir_entry_iter_to_ingestion_stream<'a, BS, I>(
    blob_service: BS,
    iter: I,
    root: &'a Path,
) -> BoxStream<'a, Result<IngestionEntry<'a>, Error>>
where
    BS: BlobService + Clone + 'a,
    I: Iterator<Item = Result<DirEntry, walkdir::Error>> + Send + 'a,
{
    let prefix = root.parent().unwrap_or_else(|| Path::new(""));

    let iter = iter.map(move |entry| match entry {
        Ok(entry) => dir_entry_to_ingestion_entry(blob_service.clone(), &entry, prefix),
        Err(error) => Err(Error::UnableToStat(
            root.to_path_buf(),
            error.into_io_error().expect("walkdir err must be some"),
        )),
    });

    Box::pin(futures::stream::iter(iter))
}

/// Converts a [walkdir::DirEntry] into an [IngestionEntry], uploading blobs to the
/// provided [BlobService].
///
/// The prefix path is stripped from the path of each entry. This is usually the parent path
/// of the path being ingested so that the last element of the stream only has one component.
fn dir_entry_to_ingestion_entry<'a, BS>(
    blob_service: BS,
    entry: &DirEntry,
    prefix: &Path,
) -> Result<IngestionEntry<'a>, Error>
where
    BS: BlobService + 'a,
{
    let file_type = entry.file_type();

    let path = entry
        .path()
        .strip_prefix(prefix)
        .expect("Tvix bug: failed to strip root path prefix")
        .to_path_buf();

    if file_type.is_dir() {
        Ok(IngestionEntry::Dir { path })
    } else if file_type.is_symlink() {
        let target = std::fs::read_link(entry.path())
            .map_err(|e| Error::UnableToStat(entry.path().to_path_buf(), e))?;

        Ok(IngestionEntry::Symlink { path, target })
    } else if file_type.is_file() {
        let metadata = entry
            .metadata()
            .map_err(|e| Error::UnableToStat(entry.path().to_path_buf(), e.into()))?;

        // TODO: In the future, for small files, hash right away and upload async.
        let digest = Box::pin(upload_blob_at_path(
            blob_service,
            entry.path().to_path_buf(),
        ));

        Ok(IngestionEntry::Regular {
            path,
            size: metadata.size(),
            // If it's executable by the user, it'll become executable.
            // This matches nix's dump() function behaviour.
            executable: metadata.permissions().mode() & 64 != 0,
            digest,
        })
    } else {
        Ok(IngestionEntry::Unknown { path, file_type })
    }
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

/// Ingests the contents at a given path into the tvix store, interacting with a [BlobService] and
/// [DirectoryService]. It returns the root node or an error.
///
/// It does not follow symlinks at the root, they will be ingested as actual symlinks.
#[instrument(skip(blob_service, directory_service), fields(path), err)]
pub async fn ingest_path<BS, DS, P>(
    blob_service: BS,
    directory_service: DS,
    path: P,
) -> Result<Node, Error>
where
    P: AsRef<Path> + std::fmt::Debug,
    BS: BlobService + Clone,
    DS: AsRef<dyn DirectoryService>,
{
    let entry_stream = walk_path_for_ingestion(blob_service, path.as_ref());
    ingest_entries(directory_service, entry_stream).await
}

/// Ingests elements from the given stream of [IngestionEntry] into a the passed [DirectoryService].
///
/// The stream must have the following invariants:
/// - All children entries must come before their parents.
/// - The last entry must be the root node which must have a single path component.
/// - Every entry should have a unique path.
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
