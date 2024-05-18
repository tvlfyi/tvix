use std::{
    io::{Cursor, Write},
    sync::Arc,
};

use tokio::{
    io::AsyncRead,
    sync::Semaphore,
    task::{JoinError, JoinSet},
};
use tokio_util::io::InspectReader;

use crate::{blobservice::BlobService, B3Digest, Path, PathBuf};

/// Files smaller than this threshold, in bytes, are uploaded to the [BlobService] in the
/// background.
///
/// This is a u32 since we acquire a weighted semaphore using the size of the blob.
/// [Semaphore::acquire_many_owned] takes a u32, so we need to ensure the size of
/// the blob can be represented using a u32 and will not cause an overflow.
const CONCURRENT_BLOB_UPLOAD_THRESHOLD: u32 = 1024 * 1024;

/// The maximum amount of bytes allowed to be buffered in memory to perform async blob uploads.
const MAX_BUFFER_SIZE: usize = 128 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unable to read blob contents for {0}: {1}")]
    BlobRead(PathBuf, std::io::Error),

    // FUTUREWORK: proper error for blob finalize
    #[error("unable to finalize blob {0}: {1}")]
    BlobFinalize(PathBuf, std::io::Error),

    #[error("unexpected size for {path} wanted: {wanted} got: {got}")]
    UnexpectedSize {
        path: PathBuf,
        wanted: u64,
        got: u64,
    },

    #[error("blob upload join error: {0}")]
    JoinError(#[from] JoinError),
}

/// The concurrent blob uploader provides a mechanism for concurrently uploading small blobs.
/// This is useful when ingesting from sources like tarballs and archives which each blob entry
/// must be read sequentially. Ingesting many small blobs sequentially becomes slow due to
/// round trip time with the blob service. The concurrent blob uploader will buffer small
/// blobs in memory and upload them to the blob service in the background.
///
/// Once all blobs have been uploaded, make sure to call [ConcurrentBlobUploader::join] to wait
/// for all background jobs to complete and check for any errors.
pub struct ConcurrentBlobUploader<BS> {
    blob_service: BS,
    upload_tasks: JoinSet<Result<(), Error>>,
    upload_semaphore: Arc<Semaphore>,
}

impl<BS> ConcurrentBlobUploader<BS>
where
    BS: BlobService + Clone + 'static,
{
    /// Creates a new concurrent blob uploader which uploads blobs to the provided
    /// blob service.
    pub fn new(blob_service: BS) -> Self {
        Self {
            blob_service,
            upload_tasks: JoinSet::new(),
            upload_semaphore: Arc::new(Semaphore::new(MAX_BUFFER_SIZE)),
        }
    }

    /// Uploads a blob to the blob service. If the blob is small enough it will be read to a buffer
    /// and uploaded in the background.
    /// This will read the entirety of the provided reader unless an error occurs, even if blobs
    /// are uploaded in the background..
    pub async fn upload<R>(
        &mut self,
        path: &Path,
        expected_size: u64,
        mut r: R,
    ) -> Result<B3Digest, Error>
    where
        R: AsyncRead + Unpin,
    {
        if expected_size < CONCURRENT_BLOB_UPLOAD_THRESHOLD as u64 {
            let mut buffer = Vec::with_capacity(expected_size as usize);
            let mut hasher = blake3::Hasher::new();
            let mut reader = InspectReader::new(&mut r, |bytes| {
                hasher.write_all(bytes).unwrap();
            });

            let permit = self
                .upload_semaphore
                .clone()
                // This cast is safe because ensure the header_size is less than
                // CONCURRENT_BLOB_UPLOAD_THRESHOLD which is a u32.
                .acquire_many_owned(expected_size as u32)
                .await
                .unwrap();
            let size = tokio::io::copy(&mut reader, &mut buffer)
                .await
                .map_err(|e| Error::BlobRead(path.into(), e))?;
            let digest: B3Digest = hasher.finalize().as_bytes().into();

            if size != expected_size {
                return Err(Error::UnexpectedSize {
                    path: path.into(),
                    wanted: expected_size,
                    got: size,
                });
            }

            self.upload_tasks.spawn({
                let blob_service = self.blob_service.clone();
                let expected_digest = digest.clone();
                let path = path.to_owned();
                let r = Cursor::new(buffer);
                async move {
                    let digest = upload_blob(&blob_service, &path, expected_size, r).await?;

                    assert_eq!(digest, expected_digest, "Tvix bug: blob digest mismatch");

                    // Make sure we hold the permit until we finish writing the blob
                    // to the [BlobService].
                    drop(permit);
                    Ok(())
                }
            });

            return Ok(digest);
        }

        upload_blob(&self.blob_service, path, expected_size, r).await
    }

    /// Waits for all background upload jobs to complete, returning any upload errors.
    pub async fn join(mut self) -> Result<(), Error> {
        while let Some(result) = self.upload_tasks.join_next().await {
            result??;
        }
        Ok(())
    }
}

async fn upload_blob<BS, R>(
    blob_service: &BS,
    path: &Path,
    expected_size: u64,
    mut r: R,
) -> Result<B3Digest, Error>
where
    BS: BlobService,
    R: AsyncRead + Unpin,
{
    let mut writer = blob_service.open_write().await;

    let size = tokio::io::copy(&mut r, &mut writer)
        .await
        .map_err(|e| Error::BlobRead(path.into(), e))?;

    let digest = writer
        .close()
        .await
        .map_err(|e| Error::BlobFinalize(path.into(), e))?;

    if size != expected_size {
        return Err(Error::UnexpectedSize {
            path: path.into(),
            wanted: expected_size,
            got: size,
        });
    }

    Ok(digest)
}
