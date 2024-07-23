use std::sync::Arc;

use tonic::async_trait;
use tracing::instrument;

use crate::composition::{CompositionContext, ServiceBuilder};
use crate::{B3Digest, Error};

use super::{BlobReader, BlobService, BlobWriter, ChunkedReader};

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

                    let chunked_reader = ChunkedReader::from_chunks(
                        remote_chunks.into_iter().map(|chunk| {
                            (
                                chunk.digest.try_into().expect("invalid b3 digest"),
                                chunk.size,
                            )
                        }),
                        Arc::new(self.clone()) as Arc<dyn BlobService>,
                    );
                    Ok(Some(Box::new(chunked_reader)))
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

#[derive(serde::Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct CombinedBlobServiceConfig {
    local: String,
    remote: String,
}

impl TryFrom<url::Url> for CombinedBlobServiceConfig {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(_url: url::Url) -> Result<Self, Self::Error> {
        Err(Error::StorageError(
            "Instantiating a CombinedBlobService from a url is not supported".into(),
        )
        .into())
    }
}

#[async_trait]
impl ServiceBuilder for CombinedBlobServiceConfig {
    type Output = dyn BlobService;
    async fn build<'a>(
        &'a self,
        _instance_name: &str,
        context: &CompositionContext,
    ) -> Result<Arc<dyn BlobService>, Box<dyn std::error::Error + Send + Sync>> {
        let (local, remote) = futures::join!(
            context.resolve(self.local.clone()),
            context.resolve(self.remote.clone())
        );
        Ok(Arc::new(CombinedBlobService {
            local: local?,
            remote: remote?,
        }))
    }
}
