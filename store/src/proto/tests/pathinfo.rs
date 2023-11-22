use crate::proto::{nar_info::Signature, NarInfo, PathInfo, ValidatePathInfoError};
use crate::tests::fixtures::*;
use bytes::Bytes;
use data_encoding::BASE64;
use nix_compat::nixbase32;
use nix_compat::store_path::{self, StorePath, StorePathRef};
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

#[test]
fn from_nixcompat_narinfo() {
    let narinfo_parsed = nix_compat::narinfo::NarInfo::parse(
        r#"StorePath: /nix/store/s66mzxpvicwk07gjbjfw9izjfa797vsw-hello-2.12.1
URL: nar/1nhgq6wcggx0plpy4991h3ginj6hipsdslv4fd4zml1n707j26yq.nar.xz
Compression: xz
FileHash: sha256:1nhgq6wcggx0plpy4991h3ginj6hipsdslv4fd4zml1n707j26yq
FileSize: 50088
NarHash: sha256:0yzhigwjl6bws649vcs2asa4lbs8hg93hyix187gc7s7a74w5h80
NarSize: 226488
References: 3n58xw4373jp0ljirf06d8077j15pc4j-glibc-2.37-8 s66mzxpvicwk07gjbjfw9izjfa797vsw-hello-2.12.1
Deriver: ib3sh3pcz10wsmavxvkdbayhqivbghlq-hello-2.12.1.drv
Sig: cache.nixos.org-1:8ijECciSFzWHwwGVOIVYdp2fOIOJAfmzGHPQVwpktfTQJF6kMPPDre7UtFw3o+VqenC5P8RikKOAAfN7CvPEAg=="#).expect("must parse");

    assert_eq!(
        PathInfo {
            node: None,
            references: vec![
                Bytes::copy_from_slice(&nixbase32::decode_fixed::<20>("3n58xw4373jp0ljirf06d8077j15pc4j").unwrap()),
                Bytes::copy_from_slice(&nixbase32::decode_fixed::<20>("s66mzxpvicwk07gjbjfw9izjfa797vsw").unwrap()),
            ],
            narinfo: Some(
                NarInfo {
                    nar_size: 226488,
                    nar_sha256: Bytes::copy_from_slice(
                        &nixbase32::decode_fixed::<32>("0yzhigwjl6bws649vcs2asa4lbs8hg93hyix187gc7s7a74w5h80".as_bytes())
                            .unwrap()
                    ),
                    signatures: vec![Signature {
                        name: "cache.nixos.org-1".to_string(),
                        data: BASE64.decode("8ijECciSFzWHwwGVOIVYdp2fOIOJAfmzGHPQVwpktfTQJF6kMPPDre7UtFw3o+VqenC5P8RikKOAAfN7CvPEAg==".as_bytes()).unwrap().into(),
                    }],
                    reference_names: vec![
                        "3n58xw4373jp0ljirf06d8077j15pc4j-glibc-2.37-8".to_string(),
                        "s66mzxpvicwk07gjbjfw9izjfa797vsw-hello-2.12.1".to_string()
                    ],
                    deriver: Some(crate::proto::StorePath {
                        digest: Bytes::copy_from_slice(&nixbase32::decode_fixed::<20>("ib3sh3pcz10wsmavxvkdbayhqivbghlq").unwrap()),
                        name: "hello-2.12.1".to_string(),
                     }),
                    ca: None,
                }
            )
        },
        (&narinfo_parsed).into(),
    );
}

#[test]
fn from_nixcompat_narinfo_fod() {
    let narinfo_parsed = nix_compat::narinfo::NarInfo::parse(
        r#"StorePath: /nix/store/pa10z4ngm0g83kx9mssrqzz30s84vq7k-hello-2.12.1.tar.gz
URL: nar/1zjrhzhaizsrlsvdkqfl073vivmxcqnzkff4s50i0cdf541ary1r.nar.xz
Compression: xz
FileHash: sha256:1zjrhzhaizsrlsvdkqfl073vivmxcqnzkff4s50i0cdf541ary1r
FileSize: 1033524
NarHash: sha256:1lvqpbk2k1sb39z8jfxixf7p7v8sj4z6mmpa44nnmff3w1y6h8lh
NarSize: 1033416
References: 
Deriver: dyivpmlaq2km6c11i0s6bi6mbsx0ylqf-hello-2.12.1.tar.gz.drv
Sig: cache.nixos.org-1:ywnIG629nQZQhEr6/HLDrLT/mUEp5J1LC6NmWSlJRWL/nM7oGItJQUYWGLvYGhSQvHrhIuvMpjNmBNh/WWqCDg==
CA: fixed:sha256:086vqwk2wl8zfs47sq2xpjc9k066ilmb8z6dn0q6ymwjzlm196cd"#
    ).expect("must parse");

    assert_eq!(
        PathInfo {
            node: None,
            references: vec![],
            narinfo: Some(
                NarInfo {
                    nar_size: 1033416,
                    nar_sha256: Bytes::copy_from_slice(
                        &nixbase32::decode_fixed::<32>(
                            "1lvqpbk2k1sb39z8jfxixf7p7v8sj4z6mmpa44nnmff3w1y6h8lh"
                        )
                        .unwrap()
                    ),
                    signatures: vec![Signature {
                        name: "cache.nixos.org-1".to_string(),
                        data: BASE64
                            .decode("ywnIG629nQZQhEr6/HLDrLT/mUEp5J1LC6NmWSlJRWL/nM7oGItJQUYWGLvYGhSQvHrhIuvMpjNmBNh/WWqCDg==".as_bytes())
                            .unwrap()
                            .into(),
                    }],
                    reference_names: vec![],
                    deriver: Some(crate::proto::StorePath {
                        digest: Bytes::copy_from_slice(
                            &nixbase32::decode_fixed::<20>("dyivpmlaq2km6c11i0s6bi6mbsx0ylqf").unwrap()
                        ),
                        name: "hello-2.12.1.tar.gz".to_string(),
                    }),
                    ca: Some(crate::proto::nar_info::Ca {
                        r#type: crate::proto::nar_info::ca::Hash::FlatSha256.into(),
                        digest: Bytes::copy_from_slice(
                            &nixbase32::decode_fixed::<32>(
                                "086vqwk2wl8zfs47sq2xpjc9k066ilmb8z6dn0q6ymwjzlm196cd"
                            )
                            .unwrap()
                        )
                    }),
                }
            ),
        },
        (&narinfo_parsed).into()
    );
}

/// Exercise .as_narinfo() on a PathInfo and ensure important fields are preserved..
#[test]
fn as_narinfo() {
    let narinfo_parsed = nix_compat::narinfo::NarInfo::parse(
        r#"StorePath: /nix/store/pa10z4ngm0g83kx9mssrqzz30s84vq7k-hello-2.12.1.tar.gz
URL: nar/1zjrhzhaizsrlsvdkqfl073vivmxcqnzkff4s50i0cdf541ary1r.nar.xz
Compression: xz
FileHash: sha256:1zjrhzhaizsrlsvdkqfl073vivmxcqnzkff4s50i0cdf541ary1r
FileSize: 1033524
NarHash: sha256:1lvqpbk2k1sb39z8jfxixf7p7v8sj4z6mmpa44nnmff3w1y6h8lh
NarSize: 1033416
References: 
Deriver: dyivpmlaq2km6c11i0s6bi6mbsx0ylqf-hello-2.12.1.tar.gz.drv
Sig: cache.nixos.org-1:ywnIG629nQZQhEr6/HLDrLT/mUEp5J1LC6NmWSlJRWL/nM7oGItJQUYWGLvYGhSQvHrhIuvMpjNmBNh/WWqCDg==
CA: fixed:sha256:086vqwk2wl8zfs47sq2xpjc9k066ilmb8z6dn0q6ymwjzlm196cd"#
    ).expect("must parse");

    let path_info: PathInfo = (&narinfo_parsed).into();

    let mut narinfo_returned = path_info
        .as_narinfo(
            StorePathRef::from_bytes(b"pa10z4ngm0g83kx9mssrqzz30s84vq7k-hello-2.12.1.tar.gz")
                .expect("invalid storepath"),
        )
        .expect("must be some");
    narinfo_returned.url = "some.nar";

    assert_eq!(
        r#"StorePath: /nix/store/pa10z4ngm0g83kx9mssrqzz30s84vq7k-hello-2.12.1.tar.gz
URL: some.nar
Compression: none
NarHash: sha256:1lvqpbk2k1sb39z8jfxixf7p7v8sj4z6mmpa44nnmff3w1y6h8lh
NarSize: 1033416
References: 
Deriver: dyivpmlaq2km6c11i0s6bi6mbsx0ylqf-hello-2.12.1.tar.gz.drv
Sig: cache.nixos.org-1:ywnIG629nQZQhEr6/HLDrLT/mUEp5J1LC6NmWSlJRWL/nM7oGItJQUYWGLvYGhSQvHrhIuvMpjNmBNh/WWqCDg==
CA: fixed:sha256:086vqwk2wl8zfs47sq2xpjc9k066ilmb8z6dn0q6ymwjzlm196cd
"#,
        narinfo_returned.to_string(),
    );
}
