//! This contains test scenarios that a given [PathInfoService] needs to pass.
//! We use [rstest] and [rstest_reuse] to provide all services we want to test
//! against, and then apply this template to all test functions.

use futures::TryStreamExt;
use rstest::*;
use rstest_reuse::{self, *};

use super::PathInfoService;
use crate::pathinfoservice::redb::RedbPathInfoService;
use crate::pathinfoservice::MemoryPathInfoService;
use crate::pathinfoservice::SledPathInfoService;
use crate::proto::PathInfo;
use crate::tests::fixtures::DUMMY_PATH_DIGEST;
use tvix_castore::proto as castorepb;

use crate::pathinfoservice::test_signing_service;

mod utils;
pub use self::utils::make_grpc_path_info_service_client;

#[cfg(all(feature = "cloud", feature = "integration"))]
use self::utils::make_bigtable_path_info_service;

#[template]
#[rstest]
#[case::memory(MemoryPathInfoService::default())]
#[case::grpc({
    let (_, _, svc) = make_grpc_path_info_service_client().await;
    svc
})]
#[case::sled(SledPathInfoService::new_temporary().unwrap())]
#[case::redb(RedbPathInfoService::new_temporary().unwrap())]
#[case::signing(test_signing_service())]
#[cfg_attr(all(feature = "cloud",feature="integration"), case::bigtable(make_bigtable_path_info_service().await))]
pub fn path_info_services(#[case] svc: impl PathInfoService) {}

// FUTUREWORK: add more tests rejecting invalid PathInfo messages.
// A subset of them should also ensure references to other PathInfos, or
// elements in {Blob,Directory}Service do exist.

/// Trying to get a non-existent PathInfo should return Ok(None).
#[apply(path_info_services)]
#[tokio::test]
async fn not_found(svc: impl PathInfoService) {
    assert!(svc
        .get(DUMMY_PATH_DIGEST)
        .await
        .expect("must succeed")
        .is_none());
}

/// Put a PathInfo into the store, get it back.
#[apply(path_info_services)]
#[tokio::test]
async fn put_get(svc: impl PathInfoService) {
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
    let resp = svc.put(path_info.clone()).await.expect("must succeed");

    // expect the returned PathInfo to be equal (for now)
    // in the future, some stores might add additional fields/signatures.
    assert_eq!(path_info, resp);

    // get it back
    let resp = svc.get(DUMMY_PATH_DIGEST).await.expect("must succeed");

    assert_eq!(Some(path_info.clone()), resp);

    // Ensure the listing endpoint works, and returns the same path_info.
    // FUTUREWORK: split this, some impls might (rightfully) not support listing
    let pathinfos: Vec<PathInfo> = svc.list().try_collect().await.expect("must succeed");

    // We should get a single pathinfo back, the one we inserted.
    assert_eq!(vec![path_info], pathinfos);
}
