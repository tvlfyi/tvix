use futures::{ready, TryStreamExt};
use pin_project_lite::pin_project;
use tokio::io::{AsyncRead, AsyncSeekExt};
use tokio_stream::StreamExt;
use tokio_util::io::{ReaderStream, StreamReader};
use tracing::{instrument, warn};

use crate::B3Digest;
use std::{cmp::Ordering, pin::Pin};

use super::{BlobReader, BlobService};

pin_project! {
    /// ChunkedReader provides a chunk-aware [BlobReader], so allows reading and
    /// seeking into a blob.
    /// It internally holds a [ChunkedBlob], which is storing chunk information
    /// able to emit a reader seeked to a specific position whenever we need to seek.
    pub struct ChunkedReader<BS> {
        chunked_blob: ChunkedBlob<BS>,

        #[pin]
        r: Box<dyn AsyncRead + Unpin + Send>,

        pos: u64,
    }
}

impl<BS> ChunkedReader<BS>
where
    BS: AsRef<dyn BlobService> + Clone + 'static + Send,
{
    /// Construct a new [ChunkedReader], by retrieving a list of chunks (their
    /// blake3 digests and chunk sizes)
    pub fn from_chunks(chunks_it: impl Iterator<Item = (B3Digest, u64)>, blob_service: BS) -> Self {
        let chunked_blob = ChunkedBlob::from_iter(chunks_it, blob_service);
        let r = chunked_blob.reader_skipped_offset(0);

        Self {
            chunked_blob,
            r,
            pos: 0,
        }
    }
}

/// ChunkedReader implements BlobReader.
impl<BS> BlobReader for ChunkedReader<BS> where BS: Send + Clone + 'static + AsRef<dyn BlobService> {}

impl<BS> tokio::io::AsyncRead for ChunkedReader<BS>
where
    BS: AsRef<dyn BlobService> + Clone + 'static,
{
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        // The amount of data read can be determined by the increase
        // in the length of the slice returned by `ReadBuf::filled`.
        let filled_before = buf.filled().len();

        let this = self.project();

        ready!(this.r.poll_read(cx, buf))?;
        let bytes_read = buf.filled().len() - filled_before;
        *this.pos += bytes_read as u64;

        Ok(()).into()
    }
}

impl<BS> tokio::io::AsyncSeek for ChunkedReader<BS>
where
    BS: AsRef<dyn BlobService> + Clone + Send + 'static,
{
    #[instrument(skip(self), err(Debug))]
    fn start_seek(self: Pin<&mut Self>, position: std::io::SeekFrom) -> std::io::Result<()> {
        let total_len = self.chunked_blob.blob_length();
        let mut this = self.project();

        let absolute_offset: u64 = match position {
            std::io::SeekFrom::Start(from_start) => from_start,
            std::io::SeekFrom::End(from_end) => {
                // note from_end is i64, not u64, so this is usually negative.
                total_len.checked_add_signed(from_end).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "over/underflow while seeking",
                    )
                })?
            }
            std::io::SeekFrom::Current(from_current) => {
                // note from_end is i64, not u64, so this can be positive or negative.
                (*this.pos)
                    .checked_add_signed(from_current)
                    .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "over/underflow while seeking",
                        )
                    })?
            }
        };

        // check if the position actually did change.
        if absolute_offset != *this.pos {
            // ensure the new position still is inside the file.
            if absolute_offset > total_len {
                Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "seeked beyond EOF",
                ))?
            }

            // Update the position and the internal reader.
            *this.pos = absolute_offset;
            *this.r = this.chunked_blob.reader_skipped_offset(absolute_offset);
        }

        Ok(())
    }

    fn poll_complete(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<u64>> {
        std::task::Poll::Ready(Ok(self.pos))
    }
}

/// Holds a list of blake3 digest for individual chunks (and their sizes).
/// Is able to construct a Reader that seeked to a certain offset, which
/// is useful to construct a BlobReader (that implements AsyncSeek).
/// - the current chunk index, and a Custor<Vec<u8>> holding the data of that chunk.
struct ChunkedBlob<BS> {
    blob_service: BS,
    chunks: Vec<(u64, u64, B3Digest)>,
}

impl<BS> ChunkedBlob<BS>
where
    BS: AsRef<dyn BlobService> + Clone + 'static + Send,
{
    /// Constructs [Self] from a list of blake3 digests of chunks and their
    /// sizes, and a reference to a blob service.
    /// Initializing it with an empty list is disallowed.
    fn from_iter(chunks_it: impl Iterator<Item = (B3Digest, u64)>, blob_service: BS) -> Self {
        let mut chunks = Vec::new();
        let mut offset: u64 = 0;

        for (chunk_digest, chunk_size) in chunks_it {
            chunks.push((offset, chunk_size, chunk_digest));
            offset += chunk_size;
        }

        assert!(
            !chunks.is_empty(),
            "Chunks must be provided, don't use this for blobs without chunks"
        );

        Self {
            blob_service,
            chunks,
        }
    }

    /// Returns the length of the blob.
    fn blob_length(&self) -> u64 {
        self.chunks
            .last()
            .map(|(chunk_offset, chunk_size, _)| chunk_offset + chunk_size)
            .unwrap_or(0)
    }

    /// For a given position pos, return the chunk containing the data.
    /// In case this would range outside the blob, None is returned.
    fn get_chunk_idx_for_position(&self, pos: u64) -> Option<usize> {
        // FUTUREWORK: benchmark when to use linear search, binary_search and BTreeSet
        self.chunks
            .binary_search_by(|(chunk_start_pos, chunk_size, _)| {
                if chunk_start_pos + chunk_size <= pos {
                    Ordering::Less
                } else if *chunk_start_pos > pos {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                }
            })
            .ok()
    }

    /// Returns a stream of bytes of the data in that blob.
    /// It internally assembles a stream reading from each chunk (skipping over
    /// chunks containing irrelevant data).
    /// From the first relevant chunk, the irrelevant bytes are skipped too.
    /// The returned boxed thing does not implement AsyncSeek on its own, but
    /// ChunkedReader does.
    fn reader_skipped_offset(&self, offset: u64) -> Box<dyn tokio::io::AsyncRead + Send + Unpin> {
        if offset == self.blob_length() {
            return Box::new(std::io::Cursor::new(vec![]));
        }
        // construct a stream of all chunks starting with the given offset
        let start_chunk_idx = self
            .get_chunk_idx_for_position(offset)
            .expect("outside of blob");
        // It's ok to panic here, we can only reach this by seeking, and seeking should already reject out-of-file seeking.

        let skip_first_chunk_bytes = (offset - self.chunks[start_chunk_idx].0) as usize;

        let blob_service = self.blob_service.clone();
        let chunks: Vec<_> = self.chunks[start_chunk_idx..].to_vec();
        let readers_stream = tokio_stream::iter(chunks.into_iter().enumerate()).map(
            move |(nth_chunk, (_chunk_start_offset, _chunk_size, chunk_digest))| {
                let chunk_digest = chunk_digest.to_owned();
                let blob_service = blob_service.clone();
                async move {
                    let mut blob_reader = blob_service
                        .as_ref()
                        .open_read(&chunk_digest.to_owned())
                        .await?
                        .ok_or_else(|| {
                            warn!(chunk.digest = %chunk_digest, "chunk not found");
                            std::io::Error::new(std::io::ErrorKind::NotFound, "chunk not found")
                        })?;

                    // iff this is the first chunk in the stream, skip by skip_first_chunk_bytes
                    if nth_chunk == 0 && skip_first_chunk_bytes > 0 {
                        blob_reader
                            .seek(std::io::SeekFrom::Start(skip_first_chunk_bytes as u64))
                            .await?;
                    }
                    Ok::<_, std::io::Error>(blob_reader)
                }
            },
        );

        // convert the stream of readers to a stream of streams of byte chunks
        let bytes_streams = readers_stream.then(|elem| async { elem.await.map(ReaderStream::new) });

        // flatten into one stream of byte chunks
        let bytes_stream = bytes_streams.try_flatten();

        // convert into AsyncRead
        Box::new(StreamReader::new(Box::pin(bytes_stream)))
    }
}

#[cfg(test)]
mod test {
    use std::{io::SeekFrom, sync::Arc};

    use crate::{
        blobservice::{chunked_reader::ChunkedReader, BlobService, MemoryBlobService},
        B3Digest,
    };
    use hex_literal::hex;
    use lazy_static::lazy_static;
    use tokio::io::{AsyncReadExt, AsyncSeekExt};

    const CHUNK_1: [u8; 2] = hex!("0001");
    const CHUNK_2: [u8; 4] = hex!("02030405");
    const CHUNK_3: [u8; 1] = hex!("06");
    const CHUNK_4: [u8; 2] = hex!("0708");
    const CHUNK_5: [u8; 7] = hex!("090a0b0c0d0e0f");

    lazy_static! {
        // `[ 0 1 ] [ 2 3 4 5 ] [ 6 ] [ 7 8 ] [ 9 10 11 12 13 14 15 ]`
        pub static ref CHUNK_1_DIGEST: B3Digest = blake3::hash(&CHUNK_1).as_bytes().into();
        pub static ref CHUNK_2_DIGEST: B3Digest = blake3::hash(&CHUNK_2).as_bytes().into();
        pub static ref CHUNK_3_DIGEST: B3Digest = blake3::hash(&CHUNK_3).as_bytes().into();
        pub static ref CHUNK_4_DIGEST: B3Digest = blake3::hash(&CHUNK_4).as_bytes().into();
        pub static ref CHUNK_5_DIGEST: B3Digest = blake3::hash(&CHUNK_5).as_bytes().into();
        pub static ref BLOB_1_LIST: [(B3Digest, u64); 5] = [
            (CHUNK_1_DIGEST.clone(), 2),
            (CHUNK_2_DIGEST.clone(), 4),
            (CHUNK_3_DIGEST.clone(), 1),
            (CHUNK_4_DIGEST.clone(), 2),
            (CHUNK_5_DIGEST.clone(), 7),
        ];
    }

    use super::ChunkedBlob;

    /// ensure the start offsets are properly calculated.
    #[test]
    fn from_iter() {
        let cb = ChunkedBlob::from_iter(
            BLOB_1_LIST.clone().into_iter(),
            Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>,
        );

        assert_eq!(
            cb.chunks,
            Vec::from_iter([
                (0, 2, CHUNK_1_DIGEST.clone()),
                (2, 4, CHUNK_2_DIGEST.clone()),
                (6, 1, CHUNK_3_DIGEST.clone()),
                (7, 2, CHUNK_4_DIGEST.clone()),
                (9, 7, CHUNK_5_DIGEST.clone()),
            ])
        );
    }

    /// ensure ChunkedBlob can't be used with an empty list of chunks
    #[test]
    #[should_panic]
    fn from_iter_empty() {
        ChunkedBlob::from_iter(
            [].into_iter(),
            Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>,
        );
    }

    /// ensure the right chunk is selected
    #[test]
    fn chunk_idx_for_position() {
        let cb = ChunkedBlob::from_iter(
            BLOB_1_LIST.clone().into_iter(),
            Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>,
        );

        assert_eq!(Some(0), cb.get_chunk_idx_for_position(0), "start of blob");

        assert_eq!(
            Some(0),
            cb.get_chunk_idx_for_position(1),
            "middle of first chunk"
        );
        assert_eq!(
            Some(1),
            cb.get_chunk_idx_for_position(2),
            "beginning of second chunk"
        );

        assert_eq!(
            Some(4),
            cb.get_chunk_idx_for_position(15),
            "right before the end of the blob"
        );
        assert_eq!(
            None,
            cb.get_chunk_idx_for_position(16),
            "right outside the blob"
        );
        assert_eq!(
            None,
            cb.get_chunk_idx_for_position(100),
            "way outside the blob"
        );
    }

    /// returns a blobservice with all chunks in BLOB_1 present.
    async fn gen_blobservice_blob1() -> Arc<dyn BlobService> {
        let blob_service = Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>;

        // seed blob service with all chunks
        for blob_contents in [
            CHUNK_1.to_vec(),
            CHUNK_2.to_vec(),
            CHUNK_3.to_vec(),
            CHUNK_4.to_vec(),
            CHUNK_5.to_vec(),
        ] {
            let mut bw = blob_service.open_write().await;
            tokio::io::copy(&mut std::io::Cursor::new(blob_contents), &mut bw)
                .await
                .expect("writing blob");
            bw.close().await.expect("close blobwriter");
        }

        blob_service
    }

    #[tokio::test]
    async fn test_read() {
        let blob_service = gen_blobservice_blob1().await;
        let mut chunked_reader =
            ChunkedReader::from_chunks(BLOB_1_LIST.clone().into_iter(), blob_service);

        // read all data
        let mut buf = Vec::new();
        tokio::io::copy(&mut chunked_reader, &mut buf)
            .await
            .expect("copy");

        assert_eq!(
            hex!("000102030405060708090a0b0c0d0e0f").to_vec(),
            buf,
            "read data must match"
        );
    }

    #[tokio::test]
    async fn test_seek() {
        let blob_service = gen_blobservice_blob1().await;
        let mut chunked_reader =
            ChunkedReader::from_chunks(BLOB_1_LIST.clone().into_iter(), blob_service);

        // seek to the end
        // expect to read 0 bytes
        {
            chunked_reader
                .seek(SeekFrom::End(0))
                .await
                .expect("seek to end");

            let mut buf = Vec::new();
            chunked_reader
                .read_to_end(&mut buf)
                .await
                .expect("read to end");

            assert_eq!(hex!("").to_vec(), buf);
        }

        // seek one bytes before the end
        {
            chunked_reader.seek(SeekFrom::End(-1)).await.expect("seek");

            let mut buf = Vec::new();
            chunked_reader
                .read_to_end(&mut buf)
                .await
                .expect("read to end");

            assert_eq!(hex!("0f").to_vec(), buf);
        }

        // seek back three bytes, but using relative positioning
        // read two bytes
        {
            chunked_reader
                .seek(SeekFrom::Current(-3))
                .await
                .expect("seek");

            let mut buf = [0b0; 2];
            chunked_reader
                .read_exact(&mut buf)
                .await
                .expect("read exact");

            assert_eq!(hex!("0d0e"), buf);
        }
    }

    // seeds a blob service with only the first two chunks, reads a bit in the
    // front (which succeeds), but then tries to seek past and read more (which
    // should fail).
    #[tokio::test]
    async fn test_read_missing_chunks() {
        let blob_service = Arc::new(MemoryBlobService::default()) as Arc<dyn BlobService>;

        for blob_contents in [CHUNK_1.to_vec(), CHUNK_2.to_vec()] {
            let mut bw = blob_service.open_write().await;
            tokio::io::copy(&mut std::io::Cursor::new(blob_contents), &mut bw)
                .await
                .expect("writing blob");

            bw.close().await.expect("close blobwriter");
        }

        let mut chunked_reader =
            ChunkedReader::from_chunks(BLOB_1_LIST.clone().into_iter(), blob_service);

        // read a bit from the front (5 bytes out of 6 available)
        let mut buf = [0b0; 5];
        chunked_reader
            .read_exact(&mut buf)
            .await
            .expect("read exact");

        assert_eq!(hex!("0001020304"), buf);

        // seek 2 bytes forward, into an area where we don't have chunks
        chunked_reader
            .seek(SeekFrom::Current(2))
            .await
            .expect("seek");

        let mut buf = Vec::new();
        chunked_reader
            .read_to_end(&mut buf)
            .await
            .expect_err("must fail");

        // FUTUREWORK: check semantics on errorkinds. Should this be InvalidData
        // or NotFound?
    }
}
