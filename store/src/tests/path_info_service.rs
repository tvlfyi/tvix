use tempfile::TempDir;

use crate::proto::path_info_service_server::PathInfoService;
use crate::proto::GetPathInfoRequest;
use crate::proto::{get_path_info_request, PathInfo};
use crate::sled_path_info_service::SledPathInfoService;

use lazy_static::lazy_static;

lazy_static! {
    static ref DUMMY_OUTPUT_HASH: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00
    ];
}

/// Trying to get a non-existent PathInfo should return a not found error.
#[tokio::test]
async fn not_found() -> anyhow::Result<()> {
    let service = SledPathInfoService::new(TempDir::new()?.path().to_path_buf())?;

    let resp = service
        .get(tonic::Request::new(GetPathInfoRequest {
            by_what: Some(get_path_info_request::ByWhat::ByOutputHash(
                DUMMY_OUTPUT_HASH.to_vec(),
            )),
        }))
        .await;

    match resp {
        Err(status) => {
            assert_eq!(status.code(), tonic::Code::NotFound);
        }
        Ok(_) => panic!("must fail"),
    };

    Ok(())
}

/// Put a PathInfo into the store, get it back.
#[tokio::test]
async fn put_get() -> anyhow::Result<()> {
    let service = SledPathInfoService::new(TempDir::new()?.path().to_path_buf())?;

    let path_info = PathInfo {
        node: Some(crate::proto::Node {
            node: Some(crate::proto::node::Node::Symlink(
                crate::proto::SymlinkNode {
                    name: "00000000000000000000000000000000-foo".to_string(),
                    target: "doesntmatter".to_string(),
                },
            )),
        }),
        ..Default::default()
    };

    let resp = service.put(tonic::Request::new(path_info.clone())).await;

    assert!(resp.is_ok());
    assert_eq!(resp.expect("must succeed").into_inner(), path_info);

    let resp = service
        .get(tonic::Request::new(GetPathInfoRequest {
            by_what: Some(get_path_info_request::ByWhat::ByOutputHash(
                DUMMY_OUTPUT_HASH.to_vec(),
            )),
        }))
        .await;

    assert!(resp.is_ok());
    assert_eq!(resp.expect("must succeed").into_inner(), path_info);

    Ok(())
}
