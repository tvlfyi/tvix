use tempfile::TempDir;
use tokio_stream::StreamExt;
use tonic::Status;

use crate::proto::directory_service_server::DirectoryService;
use crate::proto::get_directory_request::ByWhat;
use crate::proto::GetDirectoryRequest;
use crate::proto::{Directory, DirectoryNode};
use crate::sled_directory_service::SledDirectoryService;
use lazy_static::lazy_static;

lazy_static! {
    static ref DIRECTORY_A: Directory = Directory::default();
    static ref DIRECTORY_B: Directory = Directory {
        directories: vec![DirectoryNode {
            name: "a".to_string(),
            digest: DIRECTORY_A.digest(),
            size: DIRECTORY_A.size(),
        }],
        ..Default::default()
    };
}

/// Send the specified GetDirectoryRequest.
/// Returns an error in the case of an error response, or an error in one of the items in the stream,
/// or a Vec<Directory> in the case of a successful request.
async fn get_directories<S: DirectoryService>(
    svc: &S,
    get_directory_request: GetDirectoryRequest,
) -> Result<Vec<Directory>, Status> {
    let resp = svc.get(tonic::Request::new(get_directory_request)).await;

    // if the response is an error itself, return the error, otherwise unpack
    let stream = match resp {
        Ok(resp) => resp,
        Err(status) => return Err(status),
    }
    .into_inner();

    let directory_results: Vec<Result<Directory, Status>> = stream.collect().await;

    // turn Vec<Result<Directory, Status> into Result<Vec<Directory>,Status>
    directory_results.into_iter().collect()
}

/// Trying to get a non-existent Directory should return a not found error.
#[tokio::test]
async fn not_found() -> anyhow::Result<()> {
    let service = SledDirectoryService::new(TempDir::new()?.path().to_path_buf())?;

    let resp = service
        .get(tonic::Request::new(GetDirectoryRequest {
            by_what: Some(ByWhat::Digest(DIRECTORY_A.digest())),
            ..Default::default()
        }))
        .await;

    let mut rx = resp.expect("must succeed").into_inner().into_inner();

    // The stream should contain one element, an error with Code::NotFound.
    let item = rx
        .recv()
        .await
        .expect("must be some")
        .expect_err("must be err");
    assert_eq!(item.code(), tonic::Code::NotFound);

    // … and nothing else
    assert!(rx.recv().await.is_none());

    Ok(())
}

/// Put a Directory into the store, get it back.
#[tokio::test]
async fn put_get() -> anyhow::Result<()> {
    let service = SledDirectoryService::new(TempDir::new()?.path().to_path_buf())?;

    let streaming_request = tonic_mock::streaming_request(vec![DIRECTORY_A.clone()]);
    let put_resp = service
        .put(streaming_request)
        .await
        .expect("must succeed")
        .into_inner();

    // the sent root_digest should match the calculated digest
    assert_eq!(put_resp.root_digest, DIRECTORY_A.digest());

    // get it back
    let items = get_directories(
        &service,
        GetDirectoryRequest {
            by_what: Some(ByWhat::Digest(DIRECTORY_A.digest().to_vec())),
            ..Default::default()
        },
    )
    .await
    .expect("must not error");

    assert_eq!(vec![DIRECTORY_A.clone()], items);

    Ok(())
}

/// Put multiple Directories into the store, and get them back
#[tokio::test]
async fn put_get_multiple() -> anyhow::Result<()> {
    let service = SledDirectoryService::new(TempDir::new()?.path().to_path_buf())?;

    // sending "b" (which refers to "a") without sending "a" first should fail.
    let put_resp = service
        .put(tonic_mock::streaming_request(vec![DIRECTORY_B.clone()]))
        .await
        .expect_err("must fail");

    assert_eq!(tonic::Code::InvalidArgument, put_resp.code());

    // sending "a", then "b" should succeed, and the response should contain the digest of b.
    let put_resp = service
        .put(tonic_mock::streaming_request(vec![
            DIRECTORY_A.clone(),
            DIRECTORY_B.clone(),
        ]))
        .await
        .expect("must succeed");

    assert_eq!(DIRECTORY_B.digest(), put_resp.into_inner().root_digest);

    // now, request b, first in non-recursive mode.
    let items = get_directories(
        &service,
        GetDirectoryRequest {
            recursive: false,
            by_what: Some(ByWhat::Digest(DIRECTORY_B.digest())),
        },
    )
    .await
    .expect("must not error");

    // We expect to only get b.
    assert_eq!(vec![DIRECTORY_B.clone()], items);

    // now, request b, but in recursive mode.
    let items = get_directories(
        &service,
        GetDirectoryRequest {
            recursive: true,
            by_what: Some(ByWhat::Digest(DIRECTORY_B.digest())),
        },
    )
    .await
    .expect("must not error");

    // We expect to get b, and then a, because that's how we traverse down.
    assert_eq!(vec![DIRECTORY_B.clone(), DIRECTORY_A.clone()], items);

    Ok(())
}
