use crate::fixtures::{BLOB_A, BLOB_A_DIGEST};
use crate::proto::{BlobChunk, ReadBlobRequest, StatBlobRequest};
use crate::utils::gen_blobsvc_grpc_client;
use tokio_stream::StreamExt;

/// Trying to read a non-existent blob should return a not found error.
#[tokio::test]
async fn not_found_read() {
    let mut grpc_client = gen_blobsvc_grpc_client().await;

    let resp = grpc_client
        .read(ReadBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
        })
        .await;

    // We can't use unwrap_err here, because the Ok value doesn't implement
    // debug.
    if let Err(e) = resp {
        assert_eq!(e.code(), tonic::Code::NotFound);
    } else {
        panic!("resp is not err")
    }
}

/// Trying to stat a non-existent blob should return a not found error.
#[tokio::test]
async fn not_found_stat() {
    let mut grpc_client = gen_blobsvc_grpc_client().await;

    let resp = grpc_client
        .stat(StatBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
            ..Default::default()
        })
        .await
        .expect_err("must fail");

    // The resp should be a status with Code::NotFound
    assert_eq!(resp.code(), tonic::Code::NotFound);
}

/// Put a blob in the store, get it back.
#[tokio::test]
async fn put_read_stat() {
    let mut grpc_client = gen_blobsvc_grpc_client().await;

    // Send blob A.
    let put_resp = grpc_client
        .put(tokio_stream::once(BlobChunk {
            data: BLOB_A.clone(),
        }))
        .await
        .expect("must succeed")
        .into_inner();

    assert_eq!(BLOB_A_DIGEST.as_slice(), put_resp.digest);

    // Stat for the digest of A.
    // We currently don't ask for more granular chunking data, as we don't
    // expose it yet.
    let _resp = grpc_client
        .stat(StatBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
            ..Default::default()
        })
        .await
        .expect("must succeed")
        .into_inner();

    // Read the blob. It should return the same data.
    let resp = grpc_client
        .read(ReadBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
        })
        .await;

    let mut rx = resp.ok().unwrap().into_inner();

    // the stream should contain one element, a BlobChunk with the same contents as BLOB_A.
    let item = rx
        .next()
        .await
        .expect("must be some")
        .expect("must succeed");

    assert_eq!(BLOB_A.clone(), item.data);

    // … and no more elements
    assert!(rx.next().await.is_none());

    // TODO: we rely here on the blob being small enough to not get broken up into multiple chunks.
    // Test with some bigger blob too
}
