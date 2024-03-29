use crate::proto::get_path_info_request::ByWhat::ByOutputHash;
use crate::proto::path_info_service_server::PathInfoService as GRPCPathInfoService;
use crate::proto::GRPCPathInfoServiceWrapper;
use crate::proto::GetPathInfoRequest;
use crate::proto::PathInfo;
use crate::tests::fixtures::DUMMY_OUTPUT_HASH;
use crate::tests::utils::gen_blob_service;
use crate::tests::utils::gen_directory_service;
use crate::tests::utils::gen_pathinfo_service;
use futures::stream::BoxStream;
use std::sync::Arc;
use tonic::Request;
use tvix_castore::proto as castorepb;

/// generates a GRPCPathInfoService out of blob, directory and pathinfo services.
///
/// We only interact with it via the PathInfo GRPC interface.
/// It uses the NonCachingNARCalculationService NARCalculationService to
/// calculate NARs.
fn gen_grpc_service(
) -> Arc<dyn GRPCPathInfoService<ListStream = BoxStream<'static, Result<PathInfo, tonic::Status>>>>
{
    let blob_service = gen_blob_service();
    let directory_service = gen_directory_service();
    Arc::new(GRPCPathInfoServiceWrapper::new(gen_pathinfo_service(
        blob_service,
        directory_service,
    )))
}

/// Trying to get a non-existent PathInfo should return a not found error.
#[tokio::test]
async fn not_found() {
    let service = gen_grpc_service();

    let resp = service
        .get(Request::new(GetPathInfoRequest {
            by_what: Some(ByOutputHash(DUMMY_OUTPUT_HASH.clone())),
        }))
        .await;

    let resp = resp.expect_err("must fail");
    assert_eq!(resp.code(), tonic::Code::NotFound);
}

/// Put a PathInfo into the store, get it back.
#[tokio::test]
async fn put_get() {
    let service = gen_grpc_service();

    let path_info = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Symlink(castorepb::SymlinkNode {
                name: "00000000000000000000000000000000-foo".into(),
                target: "doesntmatter".into(),
            })),
        }),
        ..Default::default()
    };

    let resp = service.put(Request::new(path_info.clone())).await;

    assert!(resp.is_ok());
    assert_eq!(resp.expect("must succeed").into_inner(), path_info);

    let resp = service
        .get(Request::new(GetPathInfoRequest {
            by_what: Some(ByOutputHash(DUMMY_OUTPUT_HASH.clone())),
        }))
        .await;

    assert!(resp.is_ok());
    assert_eq!(resp.expect("must succeed").into_inner(), path_info);
}
