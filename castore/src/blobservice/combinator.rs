use data_encoding::BASE64;
use futures::{StreamExt, TryStreamExt};
use tokio_util::io::{ReaderStream, StreamReader};
use tonic::async_trait;
use tracing::{instrument, warn};

use crate::B3Digest;

use super::{naive_seeker::NaiveSeeker, BlobReader, BlobService, BlobWriter};

/// Combinator for a BlobService, using a "local" and "remote" blobservice.
/// Requests are tried in (and returned from) the local store first, only if
/// things are not present there, the remote BlobService is queried.
/// In case the local blobservice doesn't have the blob, we ask the remote
/// blobservice for chunks, and try to read each of these chunks from the local
/// blobservice again, before falling back to the remote one.
/// The remote BlobService is never written to.
pub struct CombinedBlobService<BL, BR> {
    local: BL,
    remote: BR,
}

impl<BL, BR> Clone for CombinedBlobService<BL, BR>
where
    BL: Clone,
    BR: Clone,
{
    fn clone(&self) -> Self {
        Self {
            local: self.local.clone(),
            remote: self.remote.clone(),
        }
    }
}

#[async_trait]
impl<BL, BR> BlobService for CombinedBlobService<BL, BR>
where
    BL: AsRef<dyn BlobService> + Clone + Send + Sync + 'static,
    BR: AsRef<dyn BlobService> + Clone + Send + Sync + 'static,
{
    #[instrument(skip(self, digest), fields(blob.digest=%digest))]
    async fn has(&self, digest: &B3Digest) -> std::io::Result<bool> {
        Ok(self.local.as_ref().has(digest).await? || self.remote.as_ref().has(digest).await?)
    }

    #[instrument(skip(self, digest), fields(blob.digest=%digest), err)]
    async fn open_read(&self, digest: &B3Digest) -> std::io::Result<Option<Box<dyn BlobReader>>> {
        if self.local.as_ref().has(digest).await? {
            // local store has the blob, so we can assume it also has all chunks.
            self.local.as_ref().open_read(digest).await
        } else {
            // Local store doesn't have the blob.
            // Ask the remote one for the list of chunks,
            // and create a chunked reader that uses self.open_read() for
            // individual chunks. There's a chance we already have some chunks
            // locally, meaning we don't need to fetch them all from the remote
            // BlobService.
            match self.remote.as_ref().chunks(digest).await? {
                // blob doesn't exist on the remote side either, nothing we can do.
                None => Ok(None),
                Some(remote_chunks) => {
                    // if there's no more granular chunks, or the remote
                    // blobservice doesn't support chunks, read the blob from
                    // the remote blobservice directly.
                    if remote_chunks.is_empty() {
                        return self.remote.as_ref().open_read(digest).await;
                    }
                    // otherwise, a chunked reader, which will always try the
                    // local backend first.

                    // map Vec<ChunkMeta> to Vec<(B3Digest, u64)>
                    let chunks: Vec<(B3Digest, u64)> = remote_chunks
                        .into_iter()
                        .map(|chunk_meta| {
                            (
                                B3Digest::try_from(chunk_meta.digest)
                                    .expect("invalid chunk digest"),
                                chunk_meta.size,
                            )
                        })
                        .collect();

                    Ok(Some(make_chunked_reader(self.clone(), chunks)))
                }
            }
        }
    }

    #[instrument(skip_all)]
    async fn open_write(&self) -> Box<dyn BlobWriter> {
        // direct writes to the local one.
        self.local.as_ref().open_write().await
    }
}

fn make_chunked_reader<BS>(
    // This must consume, as we can't retain references to blob_service,
    // as it'd add a lifetime to BlobReader in general, which will get
    // problematic in TvixStoreFs, which is using async move closures and cloning.
    blob_service: BS,
    // A list of b3 digests for individual chunks, and their sizes.
    chunks: Vec<(B3Digest, u64)>,
) -> Box<dyn BlobReader>
where
    BS: BlobService + Clone + 'static,
{
    // TODO: offset, verified streaming

    // construct readers for each chunk
    let blob_service = blob_service.clone();
    let readers_stream = tokio_stream::iter(chunks).map(move |(digest, _)| {
        let d = digest.to_owned();
        let blob_service = blob_service.clone();
        async move {
            blob_service.open_read(&d.to_owned()).await?.ok_or_else(|| {
                warn!(
                    chunk.digest = BASE64.encode(digest.as_slice()),
                    "chunk not found"
                );
                std::io::Error::new(std::io::ErrorKind::NotFound, "chunk not found")
            })
        }
    });

    // convert the stream of readers to a stream of streams of byte chunks
    let bytes_streams = readers_stream.then(|elem| async { elem.await.map(ReaderStream::new) });

    // flatten into one stream of byte chunks
    let bytes_stream = bytes_streams.try_flatten();

    // convert into AsyncRead
    let blob_reader = StreamReader::new(bytes_stream);

    Box::new(NaiveSeeker::new(Box::pin(blob_reader)))
}
