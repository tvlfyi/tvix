use crate::proto::{Directory, DirectoryNode, FileNode, SymlinkNode, ValidateDirectoryError};
use lazy_static::lazy_static;

mod path_info_service;
mod pathinfo;

lazy_static! {
    static ref DUMMY_DIGEST: Vec<u8> = vec![
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];
}
#[test]
fn directory_size() {
    {
        let d = Directory::default();
        assert_eq!(d.size(), 0);
    }
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: String::from("foo"),
                digest: DUMMY_DIGEST.to_vec(),
                size: 0,
            }],
            ..Default::default()
        };
        assert_eq!(d.size(), 1);
    }
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: String::from("foo"),
                digest: DUMMY_DIGEST.to_vec(),
                size: 4,
            }],
            ..Default::default()
        };
        assert_eq!(d.size(), 5);
    }
    {
        let d = Directory {
            files: vec![FileNode {
                name: String::from("foo"),
                digest: DUMMY_DIGEST.to_vec(),
                size: 42,
                executable: false,
            }],
            ..Default::default()
        };
        assert_eq!(d.size(), 1);
    }
    {
        let d = Directory {
            symlinks: vec![SymlinkNode {
                name: String::from("foo"),
                target: String::from("bar"),
            }],
            ..Default::default()
        };
        assert_eq!(d.size(), 1);
    }
}

#[test]
fn digest() {
    let d = Directory::default();

    assert_eq!(
        d.digest(),
        vec![
            0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
            0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
            0xe4, 0x1f, 0x32, 0x62
        ]
    )
}

#[test]
fn directory_validate_empty() {
    let d = Directory::default();
    assert_eq!(d.validate(), Ok(()));
}

#[test]
fn directory_validate_invalid_names() {
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: "".to_string(),
                digest: DUMMY_DIGEST.to_vec(),
                size: 42,
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidName(n) => {
                assert_eq!(n, "")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: ".".to_string(),
                digest: DUMMY_DIGEST.to_vec(),
                size: 42,
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidName(n) => {
                assert_eq!(n, ".")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            files: vec![FileNode {
                name: "..".to_string(),
                digest: DUMMY_DIGEST.to_vec(),
                size: 42,
                executable: false,
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidName(n) => {
                assert_eq!(n, "..")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            symlinks: vec![SymlinkNode {
                name: "\x00".to_string(),
                target: "foo".to_string(),
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidName(n) => {
                assert_eq!(n, "\x00")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            symlinks: vec![SymlinkNode {
                name: "foo/bar".to_string(),
                target: "foo".to_string(),
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidName(n) => {
                assert_eq!(n, "foo/bar")
            }
            _ => panic!("unexpected error"),
        };
    }
}

#[test]
fn directory_validate_invalid_digest() {
    let d = Directory {
        directories: vec![DirectoryNode {
            name: "foo".to_string(),
            digest: vec![0x00, 0x42], // invalid length
            size: 42,
        }],
        ..Default::default()
    };
    match d.validate().expect_err("must fail") {
        ValidateDirectoryError::InvalidDigestLen(n) => {
            assert_eq!(n, 2)
        }
        _ => panic!("unexpected error"),
    }
}

#[test]
fn directory_validate_sorting() {
    // "b" comes before "a", bad.
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "b".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
                DirectoryNode {
                    name: "a".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
            ],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::WrongSorting(s) => {
                assert_eq!(s, "a".to_string());
            }
            _ => panic!("unexpected error"),
        }
    }

    // "a" exists twice, bad.
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "a".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
                DirectoryNode {
                    name: "a".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
            ],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::DuplicateName(s) => {
                assert_eq!(s, "a".to_string());
            }
            _ => panic!("unexpected error"),
        }
    }

    // "a" comes before "b", all good.
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "a".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
                DirectoryNode {
                    name: "b".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
            ],
            ..Default::default()
        };

        d.validate().expect("validate shouldn't error");
    }

    // [b, c] and [a] are both properly sorted.
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "b".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
                DirectoryNode {
                    name: "c".to_string(),
                    digest: DUMMY_DIGEST.to_vec(),
                    size: 42,
                },
            ],
            symlinks: vec![SymlinkNode {
                name: "a".to_string(),
                target: "foo".to_string(),
            }],
            ..Default::default()
        };

        d.validate().expect("validate shouldn't error");
    }
}
