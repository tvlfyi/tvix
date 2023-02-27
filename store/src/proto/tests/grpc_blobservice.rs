use crate::blobservice::BlobService;
use crate::chunkservice::ChunkService;
use crate::proto::blob_meta::ChunkMeta;
use crate::proto::blob_service_server::BlobService as GRPCBlobService;
use crate::proto::{BlobChunk, GRPCBlobServiceWrapper, ReadBlobRequest, StatBlobRequest};
use crate::tests::fixtures::{BLOB_A, BLOB_A_DIGEST, BLOB_B, BLOB_B_DIGEST};
use crate::tests::utils::{gen_blob_service, gen_chunk_service};
use std::path::Path;
use tempfile::TempDir;

fn gen_grpc_blob_service(
    p: &Path,
) -> GRPCBlobServiceWrapper<
    impl BlobService + Send + Sync + Clone + 'static,
    impl ChunkService + Send + Sync + Clone + 'static,
> {
    let blob_service = gen_blob_service(p);
    let chunk_service = gen_chunk_service(p);
    GRPCBlobServiceWrapper::new(blob_service, chunk_service)
}

/// Trying to read a non-existent blob should return a not found error.
#[tokio::test]
async fn not_found_read() {
    let service = gen_grpc_blob_service(TempDir::new().unwrap().path());

    let resp = service
        .read(tonic::Request::new(ReadBlobRequest {
            digest: BLOB_A_DIGEST.to_vec(),
        }))
        .await;

    let e = resp.expect_err("must_be_err");
    assert_eq!(e.code(), tonic::Code::NotFound);
}

/// Trying to stat a non-existent blob should return a not found error.
#[tokio::test]
async fn not_found_stat() {
    let service = gen_grpc_blob_service(TempDir::new().unwrap().path());

    let resp = service
        .stat(tonic::Request::new(StatBlobRequest {
            digest: BLOB_A_DIGEST.to_vec(),
            ..Default::default()
        }))
        .await
        .expect_err("must fail");

    // The resp should be a status with Code::NotFound
    assert_eq!(resp.code(), tonic::Code::NotFound);
}

/// Put a blob in the store, get it back. We send something small enough so it
/// won't get split into multiple chunks.
#[tokio::test]
async fn put_read_stat() {
    let service = gen_grpc_blob_service(TempDir::new().unwrap().path());

    // Send blob A.
    let put_resp = service
        .put(tonic_mock::streaming_request(vec![BlobChunk {
            data: BLOB_A.clone(),
        }]))
        .await
        .expect("must succeed")
        .into_inner();

    assert_eq!(BLOB_A_DIGEST.to_vec(), put_resp.digest);

    // Stat for the digest of A. It should return one chunk.
    let resp = service
        .stat(tonic::Request::new(StatBlobRequest {
            digest: BLOB_A_DIGEST.to_vec(),
            include_chunks: true,
            ..Default::default()
        }))
        .await
        .expect("must succeed")
        .into_inner();

    assert_eq!(1, resp.chunks.len());
    // the `chunks` field should point to the single chunk.
    assert_eq!(
        vec![ChunkMeta {
            digest: BLOB_A_DIGEST.to_vec(),
            size: BLOB_A.len() as u32,
        }],
        resp.chunks,
    );

    // Read the chunk. It should return the same data.
    let resp = service
        .read(tonic::Request::new(ReadBlobRequest {
            digest: BLOB_A_DIGEST.to_vec(),
        }))
        .await;

    let mut rx = resp.expect("must succeed").into_inner().into_inner();

    // the stream should contain one element, a BlobChunk with the same contents as BLOB_A.
    let item = rx
        .recv()
        .await
        .expect("must be some")
        .expect("must succeed");

    assert_eq!(BLOB_A.to_vec(), item.data);

    // … and no more elements
    assert!(rx.recv().await.is_none());
}

/// Put a bigger blob in the store, and get it back.
/// Assert the stat request actually returns more than one chunk, and
/// we can read each chunk individually, as well as the whole blob via the
/// `read()` method.
#[tokio::test]
async fn put_read_stat_large() {
    let service = gen_grpc_blob_service(TempDir::new().unwrap().path());

    // split up BLOB_B into BlobChunks containing 1K bytes each.
    let blob_b_blobchunks: Vec<BlobChunk> = BLOB_B
        .chunks(1024)
        .map(|x| BlobChunk { data: x.to_vec() })
        .collect();

    assert!(blob_b_blobchunks.len() > 1);

    // Send blob B
    let put_resp = service
        .put(tonic_mock::streaming_request(blob_b_blobchunks))
        .await
        .expect("must succeed")
        .into_inner();

    assert_eq!(BLOB_B_DIGEST.to_vec(), put_resp.digest);

    // Stat for the digest of B
    let resp = service
        .stat(tonic::Request::new(StatBlobRequest {
            digest: BLOB_B_DIGEST.to_vec(),
            include_chunks: true,
            ..Default::default()
        }))
        .await
        .expect("must succeed")
        .into_inner();

    // it should return more than one chunk.
    assert_ne!(1, resp.chunks.len());

    // The size added up should equal the size of BLOB_B.
    let mut size_in_stat: u32 = 0;
    for chunk in &resp.chunks {
        size_in_stat += chunk.size
    }
    assert_eq!(BLOB_B.len() as u32, size_in_stat);

    // Chunks are chunked up the same way we would do locally, when initializing the chunker with the same values.
    // TODO: make the chunker config better accessible, so we don't need to synchronize this.
    {
        let chunker_avg_size = 64 * 1024;
        let chunker_min_size = chunker_avg_size / 4;
        let chunker_max_size = chunker_avg_size * 4;

        // initialize a chunker with the current buffer
        let blob_b = BLOB_B.to_vec();
        let chunker = fastcdc::v2020::FastCDC::new(
            &blob_b,
            chunker_min_size,
            chunker_avg_size,
            chunker_max_size,
        );

        let mut num_chunks = 0;
        for (i, chunk) in chunker.enumerate() {
            assert_eq!(
                resp.chunks[i].size, chunk.length as u32,
                "expected locally-chunked chunk length to match stat response"
            );

            num_chunks += 1;
        }

        assert_eq!(
            resp.chunks.len(),
            num_chunks,
            "expected number of chunks to match"
        );
    }

    // Reading the whole blob by its digest via the read() interface should succeed.
    {
        let resp = service
            .read(tonic::Request::new(ReadBlobRequest {
                digest: BLOB_B_DIGEST.to_vec(),
            }))
            .await;

        let mut rx = resp.expect("must succeed").into_inner().into_inner();

        let mut buf: Vec<u8> = Vec::new();
        while let Some(item) = rx.recv().await {
            let mut blob_chunk = item.expect("must not be err");
            buf.append(&mut blob_chunk.data);
        }

        assert_eq!(BLOB_B.to_vec(), buf);
    }

    // Reading the whole blob by reading individual chunks should also succeed.
    {
        let mut buf: Vec<u8> = Vec::new();
        for chunk in &resp.chunks {
            // request this individual chunk via read
            let resp = service
                .read(tonic::Request::new(ReadBlobRequest {
                    digest: chunk.digest.clone(),
                }))
                .await;

            let mut rx = resp.expect("must succeed").into_inner().into_inner();

            // append all items from the stream to the buffer
            while let Some(item) = rx.recv().await {
                let mut blob_chunk = item.expect("must not be err");
                buf.append(&mut blob_chunk.data);
            }
        }
        // finished looping over all chunks, compare
        assert_eq!(BLOB_B.to_vec(), buf);
    }
}
