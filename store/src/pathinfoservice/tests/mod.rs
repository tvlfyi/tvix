//! This contains test scenarios that a given [PathInfoService] needs to pass.
//! We use [rstest] and [rstest_reuse] to provide all services we want to test
//! against, and then apply this template to all test functions.

use rstest::*;
use rstest_reuse::{self, *};
use std::sync::Arc;
use tvix_castore::proto as castorepb;
use tvix_castore::{blobservice::BlobService, directoryservice::DirectoryService};

use super::PathInfoService;
use crate::proto::PathInfo;
use crate::tests::fixtures::DUMMY_OUTPUT_HASH;

mod utils;
use self::utils::make_grpc_path_info_service_client;

/// Convenience type alias batching all three servives together.
#[allow(clippy::upper_case_acronyms)]
type BSDSPS = (
    Arc<dyn BlobService>,
    Arc<dyn DirectoryService>,
    Box<dyn PathInfoService>,
);

/// Creates a PathInfoService using a new Memory{Blob,Directory}Service.
/// We return a 3-tuple containing all of them, as some tests want to interact
/// with all three.
pub async fn make_path_info_service(uri: &str) -> BSDSPS {
    let blob_service: Arc<dyn BlobService> = tvix_castore::blobservice::from_addr("memory://")
        .await
        .unwrap()
        .into();
    let directory_service: Arc<dyn DirectoryService> =
        tvix_castore::directoryservice::from_addr("memory://")
            .await
            .unwrap()
            .into();

    (
        blob_service.clone(),
        directory_service.clone(),
        crate::pathinfoservice::from_addr(uri, blob_service, directory_service)
            .await
            .unwrap(),
    )
}

#[template]
#[rstest]
#[case::memory(make_path_info_service("memory://").await)]
#[case::grpc(make_grpc_path_info_service_client().await)]
#[case::sled(make_path_info_service("sled://").await)]
pub fn path_info_services(
    #[case] services: (
        impl BlobService,
        impl DirectoryService,
        impl PathInfoService,
    ),
) {
}

// FUTUREWORK: add more tests rejecting invalid PathInfo messages.
// A subset of them should also ensure references to other PathInfos, or
// elements in {Blob,Directory}Service do exist.

/// Trying to get a non-existent PathInfo should return Ok(None).
#[apply(path_info_services)]
#[tokio::test]
async fn not_found(services: BSDSPS) {
    let (_, _, path_info_service) = services;
    assert!(path_info_service
        .get(DUMMY_OUTPUT_HASH)
        .await
        .expect("must succeed")
        .is_none());
}

/// Put a PathInfo into the store, get it back.
#[apply(path_info_services)]
#[tokio::test]
async fn put_get(services: BSDSPS) {
    let (_, _, path_info_service) = services;

    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Symlink(castorepb::SymlinkNode {
                name: "00000000000000000000000000000000-foo".into(),
                target: "doesntmatter".into(),
            })),
        }),
        ..Default::default()
    };

    // insert
    let resp = path_info_service
        .put(path_info.clone())
        .await
        .expect("must succeed");

    // expect the returned PathInfo to be equal (for now)
    // in the future, some stores might add additional fields/signatures.
    assert_eq!(path_info, resp);

    // get it back
    let resp = path_info_service
        .get(DUMMY_OUTPUT_HASH)
        .await
        .expect("must succeed");

    assert_eq!(Some(path_info), resp);
}
