use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use futures::stream::BoxStream;
use tracing::instrument;
use walkdir::DirEntry;
use walkdir::WalkDir;

use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryService;
use crate::proto::node::Node;

use super::ingest_entries;
use super::upload_blob_at_path;
use super::Error;
use super::IngestionEntry;

///! Imports that deal with a real filesystem.

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
