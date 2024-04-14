use std::{
    io::{self, Cursor},
    pin::pin,
    sync::Arc,
    task::Poll,
};

use data_encoding::HEXLOWER;
use fastcdc::v2020::AsyncStreamCDC;
use futures::Future;
use object_store::{path::Path, ObjectStore};
use pin_project_lite::pin_project;
use prost::Message;
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use tokio_stream::StreamExt;
use tonic::async_trait;
use tracing::{debug, instrument, trace, Level};
use url::Url;

use crate::{
    proto::{stat_blob_response::ChunkMeta, StatBlobResponse},
    B3Digest, B3HashingReader,
};

use super::{BlobReader, BlobService, BlobWriter, ChunkedReader};

#[derive(Clone)]
pub struct ObjectStoreBlobService {
    object_store: Arc<dyn ObjectStore>,
    base_path: Path,

    /// Average chunk size for FastCDC, in bytes.
    /// min value is half, max value double of that number.
    avg_chunk_size: u32,
}

/// Uses any object storage supported by the [object_store] crate to provide a
/// tvix-castore [BlobService].
///
/// # Data format
/// Data is organized in "blobs" and "chunks".
/// Blobs don't hold the actual data, but instead contain a list of more
/// granular chunks that assemble to the contents requested.
/// This allows clients to seek, and not download chunks they already have
/// locally, as it's referred to from other files.
/// Check `rpc_blobstore` and more general BlobStore docs on that.
///
/// ## Blobs
/// Stored at `${base_path}/blobs/b3/$digest_key`. They contains the serialized
/// StatBlobResponse for the blob with the digest.
///
/// ## Chunks
/// Chunks are stored at `${base_path}/chunks/b3/$digest_key`. They contain
/// the literal contents of the chunk, but are zstd-compressed.
///
/// ## Digest key sharding
/// The blake3 digest encoded in lower hex, and sharded after the second
/// character.
/// The blob for "Hello World" is stored at
/// `${base_path}/blobs/b3/41/41f8394111eb713a22165c46c90ab8f0fd9399c92028fd6d288944b23ff5bf76`.
///
/// This reduces the number of files in the same directory, which would be a
/// problem at least when using [object_store::local::LocalFileSystem].
///
/// # Future changes
/// There's no guarantees about this being a final format yet.
/// Once object_store gets support for additional metadata / content-types,
/// we can eliminate some requests (small blobs only consisting of a single
/// chunk can be stored as-is, without the blob index file).
/// It also allows signalling any compression of chunks in the content-type.
/// Migration *should* be possible by simply adding the right content-types to
/// all keys stored so far, but no promises ;-)
impl ObjectStoreBlobService {
    /// Constructs a new [ObjectStoreBlobService] from a [Url] supported by
    /// [object_store].
    /// Any path suffix becomes the base path of the object store.
    /// additional options, the same as in [object_store::parse_url_opts] can
    /// be passed.
    pub fn parse_url_opts<I, K, V>(url: &Url, options: I) -> Result<Self, object_store::Error>
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: Into<String>,
    {
        let (object_store, path) = object_store::parse_url_opts(url, options)?;

        Ok(Self {
            object_store: Arc::new(object_store),
            base_path: path,
            avg_chunk_size: 256 * 1024,
        })
    }

    /// Like [Self::parse_url_opts], except without the options.
    pub fn parse_url(url: &Url) -> Result<Self, object_store::Error> {
        Self::parse_url_opts(url, Vec::<(String, String)>::new())
    }
}

#[instrument(level=Level::TRACE, skip_all,fields(base_path=%base_path,blob.digest=%digest),ret(Display))]
fn derive_blob_path(base_path: &Path, digest: &B3Digest) -> Path {
    base_path
        .child("blobs")
        .child("b3")
        .child(HEXLOWER.encode(&digest.as_slice()[..2]))
        .child(HEXLOWER.encode(digest.as_slice()))
}

#[instrument(level=Level::TRACE, skip_all,fields(base_path=%base_path,chunk.digest=%digest),ret(Display))]
fn derive_chunk_path(base_path: &Path, digest: &B3Digest) -> Path {
    base_path
        .child("chunks")
        .child("b3")
        .child(HEXLOWER.encode(&digest.as_slice()[..2]))
        .child(HEXLOWER.encode(digest.as_slice()))
}

#[async_trait]
impl BlobService for ObjectStoreBlobService {
    #[instrument(skip_all, ret, err, fields(blob.digest=%digest))]
    async fn has(&self, digest: &B3Digest) -> io::Result<bool> {
        // TODO: clarify if this should work for chunks or not, and explicitly
        // document in the proto docs.
        let p = derive_blob_path(&self.base_path, digest);

        match self.object_store.head(&p).await {
            Ok(_) => Ok(true),
            Err(object_store::Error::NotFound { .. }) => {
                let p = derive_chunk_path(&self.base_path, digest);
                match self.object_store.head(&p).await {
                    Ok(_) => Ok(true),
                    Err(object_store::Error::NotFound { .. }) => Ok(false),
                    Err(e) => Err(e)?,
                }
            }
            Err(e) => Err(e)?,
        }
    }

    #[instrument(skip_all, err, fields(blob.digest=%digest))]
    async fn open_read(&self, digest: &B3Digest) -> io::Result<Option<Box<dyn BlobReader>>> {
        // handle reading the empty blob.
        if digest.as_slice() == blake3::hash(b"").as_bytes() {
            return Ok(Some(Box::new(Cursor::new(b"")) as Box<dyn BlobReader>));
        }
        match self
            .object_store
            .get(&derive_chunk_path(&self.base_path, digest))
            .await
        {
            Ok(res) => {
                // handle reading blobs that are small enough to fit inside a single chunk:
                // fetch the entire chunk into memory, decompress, ensure the b3 digest matches,
                // and return a io::Cursor over that data.
                // FUTUREWORK: use zstd::bulk to prevent decompression bombs

                let chunk_raw_bytes = res.bytes().await?;
                let chunk_contents = zstd::stream::decode_all(Cursor::new(chunk_raw_bytes))?;

                if *digest != blake3::hash(&chunk_contents).as_bytes().into() {
                    Err(io::Error::other("chunk contents invalid"))?;
                }

                Ok(Some(Box::new(Cursor::new(chunk_contents))))
            }
            Err(object_store::Error::NotFound { .. }) => {
                // NOTE: For public-facing things, we would want to stop here.
                // Clients should fetch granularly, so they can make use of
                // chunks they have locally.
                // However, if this is used directly, without any caches, do the
                // assembly here.
                // This is subject to change, once we have store composition.
                // TODO: make this configurable, and/or clarify behaviour for
                // the gRPC server surface (explicitly document behaviour in the
                // proto docs)
                if let Some(chunks) = self.chunks(digest).await? {
                    let chunked_reader = ChunkedReader::from_chunks(
                        chunks.into_iter().map(|chunk| {
                            (
                                chunk.digest.try_into().expect("invalid b3 digest"),
                                chunk.size,
                            )
                        }),
                        Arc::new(self.clone()) as Arc<dyn BlobService>,
                    );

                    Ok(Some(Box::new(chunked_reader)))
                } else {
                    // This is neither a chunk nor a blob, return None.
                    Ok(None)
                }
            }
            Err(e) => Err(e.into()),
        }
    }

    #[instrument(skip_all)]
    async fn open_write(&self) -> Box<dyn BlobWriter> {
        // ObjectStoreBlobWriter implements AsyncWrite, but all the chunking
        // needs an AsyncRead, so we create a pipe here.
        // In its `AsyncWrite` implementation, `ObjectStoreBlobWriter` delegates
        // writes to w. It periodically polls the future that's reading from the
        // other side.
        let (w, r) = tokio::io::duplex(self.avg_chunk_size as usize * 10);

        Box::new(ObjectStoreBlobWriter {
            writer: Some(w),
            fut: Some(Box::pin(chunk_and_upload(
                r,
                self.object_store.clone(),
                self.base_path.clone(),
                self.avg_chunk_size / 2,
                self.avg_chunk_size,
                self.avg_chunk_size * 2,
            ))),
            fut_output: None,
        })
    }

    #[instrument(skip_all, err, fields(blob.digest=%digest))]
    async fn chunks(&self, digest: &B3Digest) -> io::Result<Option<Vec<ChunkMeta>>> {
        match self
            .object_store
            .get(&derive_blob_path(&self.base_path, digest))
            .await
        {
            Ok(get_result) => {
                // fetch the data at the blob path
                let blob_data = get_result.bytes().await?;
                // parse into StatBlobResponse
                let stat_blob_response: StatBlobResponse = StatBlobResponse::decode(blob_data)?;

                debug!(
                    chunk.count = stat_blob_response.chunks.len(),
                    blob.size = stat_blob_response
                        .chunks
                        .iter()
                        .map(|x| x.size)
                        .sum::<u64>(),
                    "found more granular chunks"
                );

                Ok(Some(stat_blob_response.chunks))
            }
            Err(object_store::Error::NotFound { .. }) => {
                // If there's only a chunk, we must return the empty vec here, rather than None.
                match self
                    .object_store
                    .head(&derive_chunk_path(&self.base_path, digest))
                    .await
                {
                    Ok(_) => {
                        // present, but no more chunks available
                        debug!("found a single chunk");
                        Ok(Some(vec![]))
                    }
                    Err(object_store::Error::NotFound { .. }) => {
                        // Neither blob nor single chunk found
                        debug!("not found");
                        Ok(None)
                    }
                    // error checking for chunk
                    Err(e) => Err(e.into()),
                }
            }
            // error checking for blob
            Err(err) => Err(err.into()),
        }
    }
}

/// Reads blob contents from a AsyncRead, chunks and uploads them.
/// On success, returns a [StatBlobResponse] pointing to the individual chunks.
#[instrument(skip_all, fields(base_path=%base_path, min_chunk_size, avg_chunk_size, max_chunk_size), err)]
async fn chunk_and_upload<R: AsyncRead + Unpin>(
    r: R,
    object_store: Arc<dyn ObjectStore>,
    base_path: Path,
    min_chunk_size: u32,
    avg_chunk_size: u32,
    max_chunk_size: u32,
) -> io::Result<B3Digest> {
    // wrap reader with something calculating the blake3 hash of all data read.
    let mut b3_r = B3HashingReader::from(r);
    // set up a fastcdc chunker
    let mut chunker =
        AsyncStreamCDC::new(&mut b3_r, min_chunk_size, avg_chunk_size, max_chunk_size);

    /// This really should just belong into the closure at
    /// `chunker.as_stream().then(|_| { … })``, but if we try to, rustc spits
    /// higher-ranked lifetime errors at us.
    async fn fastcdc_chunk_uploader(
        resp: Result<fastcdc::v2020::ChunkData, fastcdc::v2020::Error>,
        base_path: Path,
        object_store: Arc<dyn ObjectStore>,
    ) -> std::io::Result<ChunkMeta> {
        let chunk_data = resp?;
        let chunk_digest: B3Digest = blake3::hash(&chunk_data.data).as_bytes().into();
        let chunk_path = derive_chunk_path(&base_path, &chunk_digest);

        upload_chunk(object_store, chunk_digest, chunk_path, chunk_data.data).await
    }

    // Use the fastcdc chunker to produce a stream of chunks, and upload these
    // that don't exist to the backend.
    let chunks = chunker
        .as_stream()
        .then(|resp| fastcdc_chunk_uploader(resp, base_path.clone(), object_store.clone()))
        .collect::<io::Result<Vec<ChunkMeta>>>()
        .await?;

    let stat_blob_response = StatBlobResponse {
        chunks,
        bao: "".into(), // still todo
    };

    // check for Blob, if it doesn't exist, persist.
    let blob_digest: B3Digest = b3_r.digest().into();
    let blob_path = derive_blob_path(&base_path, &blob_digest);

    match object_store.head(&blob_path).await {
        // blob already exists, nothing to do
        Ok(_) => {
            trace!(
                blob.digest = %blob_digest,
                blob.path = %blob_path,
                "blob already exists on backend"
            );
        }
        // chunk does not yet exist, upload first
        Err(object_store::Error::NotFound { .. }) => {
            debug!(
                blob.digest = %blob_digest,
                blob.path = %blob_path,
                "uploading blob"
            );
            object_store
                .put(&blob_path, stat_blob_response.encode_to_vec().into())
                .await?;
        }
        Err(err) => {
            // other error
            Err(err)?
        }
    }

    Ok(blob_digest)
}

/// upload chunk if it doesn't exist yet.
#[instrument(skip_all, fields(chunk.digest = %chunk_digest, chunk.size = chunk_data.len(), chunk.path = %chunk_path), err)]
async fn upload_chunk(
    object_store: Arc<dyn ObjectStore>,
    chunk_digest: B3Digest,
    chunk_path: Path,
    chunk_data: Vec<u8>,
) -> std::io::Result<ChunkMeta> {
    let chunk_size = chunk_data.len();
    match object_store.head(&chunk_path).await {
        // chunk already exists, nothing to do
        Ok(_) => {
            debug!("chunk already exists");
        }

        // chunk does not yet exist, compress and upload.
        Err(object_store::Error::NotFound { .. }) => {
            let chunk_data_compressed =
                zstd::encode_all(Cursor::new(chunk_data), zstd::DEFAULT_COMPRESSION_LEVEL)?;

            debug!(chunk.compressed_size=%chunk_data_compressed.len(), "uploading chunk");

            object_store
                .as_ref()
                .put(&chunk_path, chunk_data_compressed.into())
                .await?;
        }
        // other error
        Err(err) => Err(err)?,
    }

    Ok(ChunkMeta {
        digest: chunk_digest.into(),
        size: chunk_size as u64,
    })
}

pin_project! {
    /// Takes care of blob uploads.
    /// All writes are relayed to self.writer, and we continuously poll the
    /// future (which will internally read from the other side of the pipe and
    /// upload chunks).
    /// Our BlobWriter::close() needs to drop self.writer, so the other side
    /// will read EOF and can finalize the blob.
    /// The future should then resolve and return the blob digest.
    pub struct ObjectStoreBlobWriter<W, Fut>
    where
        W: AsyncWrite,
        Fut: Future,
    {
        #[pin]
        writer: Option<W>,

        #[pin]
        fut: Option<Fut>,

        fut_output: Option<io::Result<B3Digest>>
    }
}

impl<W, Fut> tokio::io::AsyncWrite for ObjectStoreBlobWriter<W, Fut>
where
    W: AsyncWrite + Send + Unpin,
    Fut: Future,
{
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, io::Error>> {
        let this = self.project();
        // poll the future.
        let fut = this.fut.as_pin_mut().expect("not future");
        let fut_p = fut.poll(cx);
        // if it's ready, the only way this could have happened is that the
        // upload failed, because we're only closing `self.writer` after all
        // writes happened.
        if fut_p.is_ready() {
            return Poll::Ready(Err(io::Error::other("upload failed")));
        }

        // write to the underlying writer
        this.writer
            .as_pin_mut()
            .expect("writer must be some")
            .poll_write(cx, buf)
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        let this = self.project();
        // poll the future.
        let fut = this.fut.as_pin_mut().expect("not future");
        let fut_p = fut.poll(cx);
        // if it's ready, the only way this could have happened is that the
        // upload failed, because we're only closing `self.writer` after all
        // writes happened.
        if fut_p.is_ready() {
            return Poll::Ready(Err(io::Error::other("upload failed")));
        }

        // Call poll_flush on the writer
        this.writer
            .as_pin_mut()
            .expect("writer must be some")
            .poll_flush(cx)
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), io::Error>> {
        // There's nothing to do on shutdown. We might have written some chunks
        // that are nowhere else referenced, but cleaning them up here would be racy.
        std::task::Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl<W, Fut> BlobWriter for ObjectStoreBlobWriter<W, Fut>
where
    W: AsyncWrite + Send + Unpin,
    Fut: Future<Output = io::Result<B3Digest>> + Send + Unpin,
{
    async fn close(&mut self) -> io::Result<B3Digest> {
        match self.writer.take() {
            Some(mut writer) => {
                // shut down the writer, so the other side will read EOF.
                writer.shutdown().await?;

                // take out the future.
                let fut = self.fut.take().expect("fut must be some");
                // await it.
                let resp = pin!(fut).await;

                match resp.as_ref() {
                    // In the case of an Ok value, we store it in self.fut_output,
                    // so future calls to close can return that.
                    Ok(b3_digest) => {
                        self.fut_output = Some(Ok(b3_digest.clone()));
                    }
                    Err(e) => {
                        // for the error type, we need to cheat a bit, as
                        // they're not clone-able.
                        // Simply store a sloppy clone, with the same ErrorKind and message there.
                        self.fut_output = Some(Err(std::io::Error::new(e.kind(), e.to_string())))
                    }
                }
                resp
            }
            None => {
                // called a second time, return self.fut_output.
                match self.fut_output.as_ref().unwrap() {
                    Ok(ref b3_digest) => Ok(b3_digest.clone()),
                    Err(e) => Err(std::io::Error::new(e.kind(), e.to_string())),
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::chunk_and_upload;
    use crate::{
        blobservice::{BlobService, ObjectStoreBlobService},
        fixtures::{BLOB_A, BLOB_A_DIGEST},
    };
    use std::{io::Cursor, sync::Arc};
    use url::Url;

    /// Tests chunk_and_upload directly, bypassing the BlobWriter at open_write().
    #[tokio::test]
    async fn test_chunk_and_upload() {
        let blobsvc = Arc::new(
            ObjectStoreBlobService::parse_url(&Url::parse("memory:///").unwrap()).unwrap(),
        );

        let blob_digest = chunk_and_upload(
            &mut Cursor::new(BLOB_A.to_vec()),
            blobsvc.object_store.clone(),
            object_store::path::Path::from("/"),
            1024 / 2,
            1024,
            1024 * 2,
        )
        .await
        .expect("chunk_and_upload succeeds");

        assert_eq!(BLOB_A_DIGEST.clone(), blob_digest);

        // Now we should have the blob
        assert!(blobsvc.has(&BLOB_A_DIGEST).await.unwrap());
    }
}
