use crate::fixtures::{DIRECTORY_A, DIRECTORY_B, DIRECTORY_C};
use crate::proto::directory_service_client::DirectoryServiceClient;
use crate::proto::get_directory_request::ByWhat;
use crate::proto::GetDirectoryRequest;
use crate::proto::{Directory, DirectoryNode, SymlinkNode};
use crate::utils::gen_directorysvc_grpc_client;
use tokio_stream::StreamExt;
use tonic::transport::Channel;
use tonic::Status;

/// Send the specified GetDirectoryRequest.
/// Returns an error in the case of an error response, or an error in one of
/// the items in the stream, or a Vec<Directory> in the case of a successful
/// request.
async fn get_directories(
    grpc_client: &mut DirectoryServiceClient<Channel>,
    get_directory_request: GetDirectoryRequest,
) -> Result<Vec<Directory>, Status> {
    let resp = grpc_client.get(get_directory_request).await;

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
async fn not_found() {
    let mut grpc_client = gen_directorysvc_grpc_client().await;

    let resp = grpc_client
        .get(GetDirectoryRequest {
            by_what: Some(ByWhat::Digest(DIRECTORY_A.digest().into())),
            ..Default::default()
        })
        .await;

    let stream = resp.expect("must succeed").into_inner();

    let items: Vec<_> = stream.collect().await;

    // The stream should contain one element, an error with Code::NotFound.
    assert_eq!(1, items.len());
    let item = items[0].clone();

    assert!(item.is_err(), "must be err");
    assert_eq!(
        tonic::Code::NotFound,
        item.unwrap_err().code(),
        "must be err"
    );
}

/// Put a Directory into the store, get it back.
#[tokio::test]
async fn put_get() {
    let mut grpc_client = gen_directorysvc_grpc_client().await;

    // send directory A.
    let put_resp = {
        grpc_client
            .put(tokio_stream::once(DIRECTORY_A.clone()))
            .await
            .expect("must succeed")
            .into_inner()
    };

    // the sent root_digest should match the calculated digest
    assert_eq!(put_resp.root_digest, DIRECTORY_A.digest().as_slice());

    // get it back
    let items = get_directories(
        &mut grpc_client,
        GetDirectoryRequest {
            by_what: Some(ByWhat::Digest(DIRECTORY_A.digest().into())),
            ..Default::default()
        },
    )
    .await
    .expect("must not error");

    assert_eq!(vec![DIRECTORY_A.clone()], items);
}

/// Put multiple Directories into the store, and get them back
#[tokio::test]
async fn put_get_multiple() {
    let mut grpc_client = gen_directorysvc_grpc_client().await;

    // sending "b" (which refers to "a") without sending "a" first should fail.
    let put_resp = {
        grpc_client
            .put(tokio_stream::once(DIRECTORY_B.clone()))
            .await
            .expect_err("must fail")
    };

    assert_eq!(tonic::Code::InvalidArgument, put_resp.code());

    // sending "a", then "b" should succeed, and the response should contain the digest of b.
    let put_resp = {
        grpc_client
            .put(tokio_stream::iter(vec![
                DIRECTORY_A.clone(),
                DIRECTORY_B.clone(),
            ]))
            .await
            .expect("must succeed")
            .into_inner()
    };

    assert_eq!(DIRECTORY_B.digest().as_slice(), put_resp.root_digest);

    // now, request b, first in non-recursive mode.
    let items = get_directories(
        &mut grpc_client,
        GetDirectoryRequest {
            recursive: false,
            by_what: Some(ByWhat::Digest(DIRECTORY_B.digest().into())),
        },
    )
    .await
    .expect("must not error");

    // We expect to only get b.
    assert_eq!(vec![DIRECTORY_B.clone()], items);

    // now, request b, but in recursive mode.
    let items = get_directories(
        &mut grpc_client,
        GetDirectoryRequest {
            recursive: true,
            by_what: Some(ByWhat::Digest(DIRECTORY_B.digest().into())),
        },
    )
    .await
    .expect("must not error");

    // We expect to get b, and then a, because that's how we traverse down.
    assert_eq!(vec![DIRECTORY_B.clone(), DIRECTORY_A.clone()], items);
}

/// Put multiple Directories into the store, and omit duplicates.
#[tokio::test]
async fn put_get_dedup() {
    let mut grpc_client = gen_directorysvc_grpc_client().await;

    // Send "A", then "C", which refers to "A" two times
    // Pretend we're a dumb client sending A twice.
    let put_resp = {
        grpc_client
            .put(tokio_stream::iter(vec![
                DIRECTORY_A.clone(),
                DIRECTORY_A.clone(),
                DIRECTORY_C.clone(),
            ]))
            .await
            .expect("must succeed")
    };

    assert_eq!(
        DIRECTORY_C.digest().as_slice(),
        put_resp.into_inner().root_digest
    );

    // Ask for "C" recursively. We expect to only get "A" once, as there's no point sending it twice.
    let items = get_directories(
        &mut grpc_client,
        GetDirectoryRequest {
            recursive: true,
            by_what: Some(ByWhat::Digest(DIRECTORY_C.digest().into())),
        },
    )
    .await
    .expect("must not error");

    // We expect to get C, and then A (once, as the second A has been deduplicated).
    assert_eq!(vec![DIRECTORY_C.clone(), DIRECTORY_A.clone()], items);
}

/// Trying to upload a Directory failing validation should fail.
#[tokio::test]
async fn put_reject_failed_validation() {
    let mut grpc_client = gen_directorysvc_grpc_client().await;

    // construct a broken Directory message that fails validation
    let broken_directory = Directory {
        symlinks: vec![SymlinkNode {
            name: "".into(),
            target: "doesntmatter".into(),
        }],
        ..Default::default()
    };
    assert!(broken_directory.validate().is_err());

    // send it over, it must fail
    let put_resp = {
        grpc_client
            .put(tokio_stream::once(broken_directory))
            .await
            .expect_err("must fail")
    };

    assert_eq!(put_resp.code(), tonic::Code::InvalidArgument);
}

/// Trying to upload a Directory with wrong size should fail.
#[tokio::test]
async fn put_reject_wrong_size() {
    let mut grpc_client = gen_directorysvc_grpc_client().await;

    // Construct a directory referring to DIRECTORY_A, but with wrong size.
    let broken_parent_directory = Directory {
        directories: vec![DirectoryNode {
            name: "foo".into(),
            digest: DIRECTORY_A.digest().into(),
            size: 42,
        }],
        ..Default::default()
    };
    // Make sure we got the size wrong.
    assert_ne!(
        broken_parent_directory.directories[0].size,
        DIRECTORY_A.size()
    );

    // now upload both (first A, then the broken parent). This must fail.
    let put_resp = {
        grpc_client
            .put(tokio_stream::iter(vec![
                DIRECTORY_A.clone(),
                broken_parent_directory,
            ]))
            .await
            .expect_err("must fail")
    };
    assert_eq!(put_resp.code(), tonic::Code::InvalidArgument);
}
