use crate::proto::{PathInfo, ValidatePathInfoError};
use crate::tests::fixtures::*;
use bytes::Bytes;
use nix_compat::store_path::{self, StorePath};
use std::str::FromStr;
use test_case::test_case;
use tvix_castore::proto as castorepb;

#[test_case(
    None,
    Err(ValidatePathInfoError::NoNodePresent) ;
    "No node"
)]
#[test_case(
    Some(castorepb::Node { node: None }),
    Err(ValidatePathInfoError::NoNodePresent);
    "No node 2"
)]
fn validate_no_node(
    t_node: Option<castorepb::Node>,
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
    castorepb::DirectoryNode {
        name: DUMMY_NAME.into(),
        digest: DUMMY_DIGEST.clone().into(),
        size: 0,
    },
    Ok(StorePath::from_str(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    castorepb::DirectoryNode {
        name: DUMMY_NAME.into(),
        digest: Bytes::new(),
        size: 0,
    },
    Err(ValidatePathInfoError::InvalidRootNode(castorepb::ValidateNodeError::InvalidDigestLen(0)));
    "invalid digest length"
)]
#[test_case(
    castorepb::DirectoryNode {
        name: "invalid".into(),
        digest: DUMMY_DIGEST.clone().into(),
        size: 0,
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength
    ));
    "invalid node name"
)]
fn validate_directory(
    t_directory_node: castorepb::DirectoryNode,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Directory(t_directory_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    castorepb::FileNode {
        name: DUMMY_NAME.into(),
        digest: DUMMY_DIGEST.clone().into(),
        size: 0,
        executable: false,
    },
    Ok(StorePath::from_str(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    castorepb::FileNode {
        name: DUMMY_NAME.into(),
        digest: Bytes::new(),
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidRootNode(castorepb::ValidateNodeError::InvalidDigestLen(0)));
    "invalid digest length"
)]
#[test_case(
    castorepb::FileNode {
        name: "invalid".into(),
        digest: DUMMY_DIGEST.clone().into(),
        ..Default::default()
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength
    ));
    "invalid node name"
)]
fn validate_file(
    t_file_node: castorepb::FileNode,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::File(t_file_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

#[test_case(
    castorepb::SymlinkNode {
        name: DUMMY_NAME.into(),
        target: "foo".into(),
    },
    Ok(StorePath::from_str(DUMMY_NAME).expect("must succeed"));
    "ok"
)]
#[test_case(
    castorepb::SymlinkNode {
        name: "invalid".into(),
        target: "foo".into(),
    },
    Err(ValidatePathInfoError::InvalidNodeName(
        "invalid".into(),
        store_path::Error::InvalidLength
    ));
    "invalid node name"
)]
fn validate_symlink(
    t_symlink_node: castorepb::SymlinkNode,
    t_result: Result<StorePath, ValidatePathInfoError>,
) {
    // construct the PathInfo object
    let p = PathInfo {
        node: Some(castorepb::Node {
            node: Some(castorepb::node::Node::Symlink(t_symlink_node)),
        }),
        ..Default::default()
    };
    assert_eq!(t_result, p.validate());
}

/// Ensure parsing a correct PathInfo without narinfo populated succeeds.
#[test]
fn validate_references_without_narinfo_ok() {
    assert!(PATH_INFO_WITHOUT_NARINFO.validate().is_ok());
}

/// Ensure parsing a correct PathInfo with narinfo populated succeeds.
#[test]
fn validate_references_with_narinfo_ok() {
    assert!(PATH_INFO_WITH_NARINFO.validate().is_ok());
}

/// Create a PathInfo with a wrong digest length in narinfo.nar_sha256, and
/// ensure validation fails.
#[test]
fn validate_wrong_nar_sha256() {
    let mut path_info = PATH_INFO_WITH_NARINFO.clone();
    path_info.narinfo.as_mut().unwrap().nar_sha256 = vec![0xbe, 0xef].into();

    match path_info.validate().expect_err("must_fail") {
        ValidatePathInfoError::InvalidNarSha256DigestLen(2) => {}
        e => panic!("unexpected error: {:?}", e),
    };
}

/// Create a PathInfo with a wrong count of narinfo.reference_names,
/// and ensure validation fails.
#[test]
fn validate_inconsistent_num_refs_fail() {
    let mut path_info = PATH_INFO_WITH_NARINFO.clone();
    path_info.narinfo.as_mut().unwrap().reference_names = vec![];

    match path_info.validate().expect_err("must_fail") {
        ValidatePathInfoError::InconsistentNumberOfReferences(1, 0) => {}
        e => panic!("unexpected error: {:?}", e),
    };
}

/// Create a PathInfo with a wrong digest length in references.
#[test]
fn validate_invalid_reference_digest_len() {
    let mut path_info = PATH_INFO_WITHOUT_NARINFO.clone();
    path_info.references.push(vec![0xff, 0xff].into());

    match path_info.validate().expect_err("must fail") {
        ValidatePathInfoError::InvalidReferenceDigestLen(
            1, // position
            2, // unexpected digest len
        ) => {}
        e => panic!("unexpected error: {:?}", e),
    };
}

/// Create a PathInfo with a narinfo.reference_name[1] that is no valid store path.
#[test]
fn validate_invalid_narinfo_reference_name() {
    let mut path_info = PATH_INFO_WITH_NARINFO.clone();

    // This is invalid, as the store prefix is not part of reference_names.
    path_info.narinfo.as_mut().unwrap().reference_names[0] =
        "/nix/store/00000000000000000000000000000000-dummy".to_string();

    match path_info.validate().expect_err("must fail") {
        ValidatePathInfoError::InvalidNarinfoReferenceName(0, reference_name) => {
            assert_eq!(
                "/nix/store/00000000000000000000000000000000-dummy",
                reference_name
            );
        }
        e => panic!("unexpected error: {:?}", e),
    }
}

/// Create a PathInfo with a narinfo.reference_name[0] that doesn't match references[0].
#[test]
fn validate_inconsistent_narinfo_reference_name_digest() {
    let mut path_info = PATH_INFO_WITH_NARINFO.clone();

    // mutate the first reference, they were all zeroes before
    path_info.references[0] = vec![0xff; store_path::DIGEST_SIZE].into();

    match path_info.validate().expect_err("must fail") {
        ValidatePathInfoError::InconsistentNarinfoReferenceNameDigest(0, e_expected, e_actual) => {
            assert_eq!(path_info.references[0][..], e_expected);
            assert_eq!(DUMMY_OUTPUT_HASH[..], e_actual);
        }
        e => panic!("unexpected error: {:?}", e),
    }
}

/// Create a node with an empty symlink target, and ensure it fails validation.
#[test]
fn validate_symlink_empty_target_invalid() {
    let node = castorepb::node::Node::Symlink(castorepb::SymlinkNode {
        name: "foo".into(),
        target: "".into(),
    });

    node.validate().expect_err("must fail validation");
}

/// Create a node with a symlink target including null bytes, and ensure it
/// fails validation.
#[test]
fn validate_symlink_target_null_byte_invalid() {
    let node = castorepb::node::Node::Symlink(castorepb::SymlinkNode {
        name: "foo".into(),
        target: "foo\0".into(),
    });

    node.validate().expect_err("must fail validation");
}

/// Create a PathInfo with a correct deriver field and ensure it succeeds.
#[test]
fn validate_valid_deriver() {
    let mut path_info = PATH_INFO_WITH_NARINFO.clone();

    // add a valid deriver
    let narinfo = path_info.narinfo.as_mut().unwrap();
    narinfo.deriver = Some(crate::proto::StorePath {
        name: "foo".to_string(),
        digest: DUMMY_OUTPUT_HASH.clone(),
    });

    path_info.validate().expect("must validate");
}

/// Create a PathInfo with a broken deriver field and ensure it fails.
#[test]
fn validate_invalid_deriver() {
    let mut path_info = PATH_INFO_WITH_NARINFO.clone();

    // add a broken deriver (invalid digest)
    let narinfo = path_info.narinfo.as_mut().unwrap();
    narinfo.deriver = Some(crate::proto::StorePath {
        name: "foo".to_string(),
        digest: vec![].into(),
    });

    match path_info.validate().expect_err("must fail validation") {
        ValidatePathInfoError::InvalidDeriverField(_) => {}
        e => panic!("unexpected error: {:?}", e),
    }
}
