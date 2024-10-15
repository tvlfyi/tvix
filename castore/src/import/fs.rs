//! Import from a real filesystem.

use futures::stream::BoxStream;
use futures::StreamExt;
use std::fs::FileType;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::PermissionsExt;
use tokio::io::BufReader;
use tokio_util::io::InspectReader;
use tracing::info_span;
use tracing::instrument;
use tracing::Instrument;
use tracing::Span;
use tracing_indicatif::span_ext::IndicatifSpanExt;
use walkdir::DirEntry;
use walkdir::WalkDir;

use crate::blobservice::BlobService;
use crate::directoryservice::DirectoryService;
use crate::refscan::{ReferenceReader, ReferenceScanner};
use crate::{B3Digest, Node};

use super::ingest_entries;
use super::IngestionEntry;
use super::IngestionError;

/// Ingests the contents at a given path into the tvix store, interacting with a [BlobService] and
/// [DirectoryService]. It returns the root node or an error.
///
/// It does not follow symlinks at the root, they will be ingested as actual symlinks.
///
/// This function will walk the filesystem using `walkdir` and will consume
/// `O(#number of entries)` space.
#[instrument(
    skip(blob_service, directory_service, reference_scanner),
    fields(path),
    err
)]
pub async fn ingest_path<BS, DS, P, P2>(
    blob_service: BS,
    directory_service: DS,
    path: P,
    reference_scanner: Option<&ReferenceScanner<P2>>,
) -> Result<Node, IngestionError<Error>>
where
    P: AsRef<std::path::Path> + std::fmt::Debug,
    BS: BlobService + Clone,
    DS: DirectoryService,
    P2: AsRef<[u8]> + Send + Sync,
{
    let span = Span::current();

    let iter = WalkDir::new(path.as_ref())
        .follow_links(false)
        .follow_root_links(false)
        .contents_first(true)
        .into_iter();

    let entries =
        dir_entries_to_ingestion_stream(blob_service, iter, path.as_ref(), reference_scanner);
    ingest_entries(
        directory_service,
        entries.inspect({
            let span = span.clone();
            move |e| {
                if e.is_ok() {
                    span.pb_inc(1)
                }
            }
        }),
    )
    .await
}

/// Converts an iterator of [walkdir::DirEntry]s into a stream of ingestion entries.
/// This can then be fed into [ingest_entries] to ingest all the entries into the castore.
///
/// The produced stream is buffered, so uploads can happen concurrently.
///
/// The root is the [Path] in the filesystem that is being ingested into the castore.
pub fn dir_entries_to_ingestion_stream<'a, BS, I, P>(
    blob_service: BS,
    iter: I,
    root: &'a std::path::Path,
    reference_scanner: Option<&'a ReferenceScanner<P>>,
) -> BoxStream<'a, Result<IngestionEntry, Error>>
where
    BS: BlobService + Clone + 'a,
    I: Iterator<Item = Result<DirEntry, walkdir::Error>> + Send + 'a,
    P: AsRef<[u8]> + Send + Sync,
{
    let prefix = root.parent().unwrap_or_else(|| std::path::Path::new(""));

    Box::pin(
        futures::stream::iter(iter)
            .map(move |x| {
                let blob_service = blob_service.clone();
                async move {
                    match x {
                        Ok(dir_entry) => {
                            dir_entry_to_ingestion_entry(
                                blob_service,
                                &dir_entry,
                                prefix,
                                reference_scanner,
                            )
                            .await
                        }
                        Err(e) => Err(Error::Stat(
                            prefix.to_path_buf(),
                            e.into_io_error().expect("walkdir err must be some"),
                        )),
                    }
                }
            })
            .buffered(50),
    )
}

/// Converts a [walkdir::DirEntry] into an [IngestionEntry], uploading blobs to the
/// provided [BlobService].
///
/// The prefix path is stripped from the path of each entry. This is usually the parent path
/// of the path being ingested so that the last element of the stream only has one component.
pub async fn dir_entry_to_ingestion_entry<BS, P>(
    blob_service: BS,
    entry: &DirEntry,
    prefix: &std::path::Path,
    reference_scanner: Option<&ReferenceScanner<P>>,
) -> Result<IngestionEntry, Error>
where
    BS: BlobService,
    P: AsRef<[u8]>,
{
    let file_type = entry.file_type();

    let fs_path = entry
        .path()
        .strip_prefix(prefix)
        .expect("Tvix bug: failed to strip root path prefix");

    // convert to castore PathBuf
    let path = crate::path::PathBuf::from_host_path(fs_path, false)
        .unwrap_or_else(|e| panic!("Tvix bug: walkdir direntry cannot be parsed: {}", e));

    if file_type.is_dir() {
        Ok(IngestionEntry::Dir { path })
    } else if file_type.is_symlink() {
        let target = std::fs::read_link(entry.path())
            .map_err(|e| Error::Stat(entry.path().to_path_buf(), e))?
            .into_os_string()
            .into_vec();

        if let Some(reference_scanner) = &reference_scanner {
            reference_scanner.scan(&target);
        }

        Ok(IngestionEntry::Symlink { path, target })
    } else if file_type.is_file() {
        let metadata = entry
            .metadata()
            .map_err(|e| Error::Stat(entry.path().to_path_buf(), e.into()))?;

        let digest = upload_blob(blob_service, entry.path().to_path_buf(), reference_scanner)
            .instrument({
                let span = info_span!("upload_blob", "indicatif.pb_show" = tracing::field::Empty);
                span.pb_set_message(&format!("Uploading blob for {:?}", fs_path));
                span.pb_set_style(&tvix_tracing::PB_TRANSFER_STYLE);

                span
            })
            .await?;

        Ok(IngestionEntry::Regular {
            path,
            size: metadata.size(),
            // If it's executable by the user, it'll become executable.
            // This matches nix's dump() function behaviour.
            executable: metadata.permissions().mode() & 64 != 0,
            digest,
        })
    } else {
        return Err(Error::FileType(fs_path.to_path_buf(), file_type));
    }
}

/// Uploads the file at the provided [Path] the the [BlobService].
#[instrument(skip(blob_service, reference_scanner), fields(path), err)]
async fn upload_blob<BS, P>(
    blob_service: BS,
    path: impl AsRef<std::path::Path>,
    reference_scanner: Option<&ReferenceScanner<P>>,
) -> Result<B3Digest, Error>
where
    BS: BlobService,
    P: AsRef<[u8]>,
{
    let span = Span::current();
    span.pb_start();

    let file = tokio::fs::File::open(path.as_ref())
        .await
        .map_err(|e| Error::BlobRead(path.as_ref().to_path_buf(), e))?;

    let metadata = file
        .metadata()
        .await
        .map_err(|e| Error::Stat(path.as_ref().to_path_buf(), e))?;

    span.pb_set_length(metadata.len());
    let reader = InspectReader::new(file, |d| {
        span.pb_inc(d.len() as u64);
    });

    let mut writer = blob_service.open_write().await;
    if let Some(reference_scanner) = reference_scanner {
        let mut reader = ReferenceReader::new(reference_scanner, BufReader::new(reader));
        tokio::io::copy(&mut reader, &mut writer)
            .await
            .map_err(|e| Error::BlobRead(path.as_ref().to_path_buf(), e))?;
    } else {
        tokio::io::copy(&mut BufReader::new(reader), &mut writer)
            .await
            .map_err(|e| Error::BlobRead(path.as_ref().to_path_buf(), e))?;
    }

    let digest = writer
        .close()
        .await
        .map_err(|e| Error::BlobFinalize(path.as_ref().to_path_buf(), e))?;

    Ok(digest)
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported file type at {0}: {1:?}")]
    FileType(std::path::PathBuf, FileType),

    #[error("unable to stat {0}: {1}")]
    Stat(std::path::PathBuf, std::io::Error),

    #[error("unable to open {0}: {1}")]
    Open(std::path::PathBuf, std::io::Error),

    #[error("unable to read {0}: {1}")]
    BlobRead(std::path::PathBuf, std::io::Error),

    // TODO: proper error for blob finalize
    #[error("unable to finalize blob {0}: {1}")]
    BlobFinalize(std::path::PathBuf, std::io::Error),
}
