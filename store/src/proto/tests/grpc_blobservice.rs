use crate::proto::blob_service_server::BlobService as GRPCBlobService;
use crate::proto::{BlobChunk, GRPCBlobServiceWrapper, ReadBlobRequest, StatBlobRequest};
use crate::tests::fixtures::{BLOB_A, BLOB_A_DIGEST};
use crate::tests::utils::gen_blob_service;
use tokio_stream::StreamExt;

fn gen_grpc_blob_service() -> GRPCBlobServiceWrapper {
    let blob_service = gen_blob_service();
    GRPCBlobServiceWrapper::from(blob_service)
}

/// Trying to read a non-existent blob should return a not found error.
#[tokio::test]
async fn not_found_read() {
    let service = gen_grpc_blob_service();

    let resp = service
        .read(tonic::Request::new(ReadBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
        }))
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
    let service = gen_grpc_blob_service();

    let resp = service
        .stat(tonic::Request::new(StatBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
            ..Default::default()
        }))
        .await
        .expect_err("must fail");

    // The resp should be a status with Code::NotFound
    assert_eq!(resp.code(), tonic::Code::NotFound);
}

/// Put a blob in the store, get it back.
#[tokio::test]
async fn put_read_stat() {
    let service = gen_grpc_blob_service();

    // Send blob A.
    let put_resp = service
        .put(tonic_mock::streaming_request(vec![BlobChunk {
            data: BLOB_A.clone().into(),
        }]))
        .await
        .expect("must succeed")
        .into_inner();

    assert_eq!(BLOB_A_DIGEST.to_vec(), put_resp.digest);

    // Stat for the digest of A.
    // We currently don't ask for more granular chunking data, as we don't
    // expose it yet.
    let _resp = service
        .stat(tonic::Request::new(StatBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
            ..Default::default()
        }))
        .await
        .expect("must succeed")
        .into_inner();

    // Read the blob. It should return the same data.
    let resp = service
        .read(tonic::Request::new(ReadBlobRequest {
            digest: BLOB_A_DIGEST.clone().into(),
        }))
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
