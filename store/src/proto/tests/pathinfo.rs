use crate::proto::{self, Node, PathInfo, ValidatePathInfoError};
use crate::B3Digest;
use bytes::Bytes;
use lazy_static::lazy_static;
use nix_compat::store_path::{self, StorePath};
use std::str::FromStr;
use test_case::test_case;

lazy_static! {
    static ref DUMMY_DIGEST: B3Digest = {
        let u: &[u8; 32] = &[
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        u.into()
    };
    static ref DUMMY_DIGEST_2: B3Digest = {
        let u: &[u8; 32] = &[
            0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
        u.into()
    };
}

const DUMMY_NAME: &str = "00000000000000000000000000000000-dummy";

#[test_case(
    None,
    Err(ValidatePathInfoError::NoNodePresent()) ;
    "No node"
)]
#[test_case(
    Some(Node { node: None }),
    Err(ValidatePathInfoError::NoNodePresent());
    "No node 2"
)]
fn validate_no_node(
    t_node: Option<proto::Node>,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: t_node,
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    proto::DirectoryNode {
        name: DUMMY_NAME.into(),
        digest: DUMMY_DIGEST.clone().into(),
        size: 0,
    },
    Ok(StorePath::from_str(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    proto::DirectoryNode {
        name: DUMMY_NAME.into(),
        digest: Bytes::new(),
        size: 0,
    },
    Err(ValidatePathInfoError::InvalidDigestLen(0));
    "invalid digest length"
)]
#[test_case(
    proto::DirectoryNode {
        name: "invalid".into(),
        digest: DUMMY_DIGEST.clone().into(),
        size: 0,
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength()
    ));
    "invalid node name"
)]
fn validate_directory(
    t_directory_node: proto::DirectoryNode,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::Directory(t_directory_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    proto::FileNode {
        name: DUMMY_NAME.into(),
        digest: DUMMY_DIGEST.clone().into(),
        size: 0,
        executable: false,
    },
    Ok(StorePath::from_str(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    proto::FileNode {
        name: DUMMY_NAME.into(),
        digest: Bytes::new(),
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidDigestLen(0));
    "invalid digest length"
)]
#[test_case(
    proto::FileNode {
        name: "invalid".into(),
        digest: DUMMY_DIGEST.clone().into(),
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength()
    ));
    "invalid node name"
)]
fn validate_file(t_file_node: proto::FileNode, t_result: Result<StorePath, ValidatePathInfoError>) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::File(t_file_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    proto::SymlinkNode {
        name: DUMMY_NAME.into(),
        ..Default::default()
    },
    Ok(StorePath::from_str(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    proto::SymlinkNode {
        name: "invalid".into(),
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength()
    ));
    "invalid node name"
)]
fn validate_symlink(
    t_symlink_node: proto::SymlinkNode,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::Symlink(t_symlink_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test]
fn validate_references() {
    // create a PathInfo without narinfo field.
    let path_info = PathInfo {
        node: Some(Node {
            node: Some(proto::node::Node::Directory(proto::DirectoryNode {
                name: DUMMY_NAME.into(),
                digest: DUMMY_DIGEST.clone().into(),
                size: 0,
            })),
        }),
        references: vec![DUMMY_DIGEST_2.clone().into()],
        narinfo: None,
    };
    assert!(path_info.validate().is_ok());

    // create a PathInfo with a narinfo field, but an inconsistent set of references
    let path_info_with_narinfo_missing_refs = PathInfo {
        narinfo: Some(proto::NarInfo {
            nar_size: 0,
            nar_sha256: DUMMY_DIGEST.clone().into(),
            signatures: vec![],
            reference_names: vec![],
        }),
        ..path_info.clone()
    };
    match path_info_with_narinfo_missing_refs
        .validate()
        .expect_err("must_fail")
    {
        ValidatePathInfoError::InconsistentNumberOfReferences(_, _) => {}
        _ => panic!("unexpected error"),
    };

    // create a pathinfo with the correct number of references, should suceed
    let path_info_with_narinfo = PathInfo {
        narinfo: Some(proto::NarInfo {
            nar_size: 0,
            nar_sha256: DUMMY_DIGEST.clone().into(),
            signatures: vec![],
            reference_names: vec![format!("/nix/store/{}", DUMMY_NAME)],
        }),
        ..path_info
    };
    assert!(path_info_with_narinfo.validate().is_ok());
}
