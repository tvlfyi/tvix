use crate::proto::{
    node, Directory, DirectoryNode, FileNode, SymlinkNode, ValidateDirectoryError,
    ValidateNodeError,
};

use hex_literal::hex;

const DUMMY_DIGEST: [u8; 32] = [0; 32];

#[test]
fn size() {
    {
        let d = Directory::default();
        assert_eq!(d.size(), 0);
    }
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: "foo".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
                size: 0,
            }],
            ..Default::default()
        };
        assert_eq!(d.size(), 1);
    }
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: "foo".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
                size: 4,
            }],
            ..Default::default()
        };
        assert_eq!(d.size(), 5);
    }
    {
        let d = Directory {
            files: vec![FileNode {
                name: "foo".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
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
                name: "foo".into(),
                target: "bar".into(),
            }],
            ..Default::default()
        };
        assert_eq!(d.size(), 1);
    }
}

#[test]
#[cfg_attr(not(debug_assertions), ignore)]
#[should_panic = "Directory::size exceeds u64::MAX"]
fn size_unchecked_panic() {
    let d = Directory {
        directories: vec![DirectoryNode {
            name: "foo".into(),
            digest: DUMMY_DIGEST.to_vec().into(),
            size: u64::MAX,
        }],
        ..Default::default()
    };

    d.size();
}

#[test]
#[cfg_attr(debug_assertions, ignore)]
fn size_unchecked_saturate() {
    let d = Directory {
        directories: vec![DirectoryNode {
            name: "foo".into(),
            digest: DUMMY_DIGEST.to_vec().into(),
            size: u64::MAX,
        }],
        ..Default::default()
    };

    assert_eq!(d.size(), u64::MAX);
}

#[test]
fn size_checked() {
    // We don't test the overflow cases that rely purely on immediate
    // child count, since that would take an absurd amount of memory.
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: "foo".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
                size: u64::MAX - 1,
            }],
            ..Default::default()
        };
        assert_eq!(d.size_checked(), Some(u64::MAX));
    }
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: "foo".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
                size: u64::MAX,
            }],
            ..Default::default()
        };
        assert_eq!(d.size_checked(), None);
    }
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "foo".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: u64::MAX / 2,
                },
                DirectoryNode {
                    name: "foo".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: u64::MAX / 2,
                },
            ],
            ..Default::default()
        };
        assert_eq!(d.size_checked(), None);
    }
}

#[test]
fn digest() {
    let d = Directory::default();

    assert_eq!(
        d.digest(),
        (&hex!("af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262")).into()
    )
}

#[test]
fn validate_empty() {
    let d = Directory::default();
    assert_eq!(d.validate(), Ok(()));
}

#[test]
fn validate_invalid_names() {
    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: "".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
                size: 42,
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidNode(n, ValidateNodeError::InvalidName(_)) => {
                assert_eq!(n, b"")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            directories: vec![DirectoryNode {
                name: ".".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
                size: 42,
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidNode(n, ValidateNodeError::InvalidName(_)) => {
                assert_eq!(n, b".")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            files: vec![FileNode {
                name: "..".into(),
                digest: DUMMY_DIGEST.to_vec().into(),
                size: 42,
                executable: false,
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidNode(n, ValidateNodeError::InvalidName(_)) => {
                assert_eq!(n, b"..")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            symlinks: vec![SymlinkNode {
                name: "\x00".into(),
                target: "foo".into(),
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidNode(n, ValidateNodeError::InvalidName(_)) => {
                assert_eq!(n, b"\x00")
            }
            _ => panic!("unexpected error"),
        };
    }

    {
        let d = Directory {
            symlinks: vec![SymlinkNode {
                name: "foo/bar".into(),
                target: "foo".into(),
            }],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::InvalidNode(n, ValidateNodeError::InvalidName(_)) => {
                assert_eq!(n, b"foo/bar")
            }
            _ => panic!("unexpected error"),
        };
    }
}

#[test]
fn validate_invalid_digest() {
    let d = Directory {
        directories: vec![DirectoryNode {
            name: "foo".into(),
            digest: vec![0x00, 0x42].into(), // invalid length
            size: 42,
        }],
        ..Default::default()
    };
    match d.validate().expect_err("must fail") {
        ValidateDirectoryError::InvalidNode(_, ValidateNodeError::InvalidDigestLen(n)) => {
            assert_eq!(n, 2)
        }
        _ => panic!("unexpected error"),
    }
}

#[test]
fn validate_sorting() {
    // "b" comes before "a", bad.
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "b".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: 42,
                },
                DirectoryNode {
                    name: "a".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: 42,
                },
            ],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::WrongSorting(s) => {
                assert_eq!(s, b"a");
            }
            _ => panic!("unexpected error"),
        }
    }

    // "a" exists twice, bad.
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "a".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: 42,
                },
                DirectoryNode {
                    name: "a".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: 42,
                },
            ],
            ..Default::default()
        };
        match d.validate().expect_err("must fail") {
            ValidateDirectoryError::DuplicateName(s) => {
                assert_eq!(s, b"a");
            }
            _ => panic!("unexpected error"),
        }
    }

    // "a" comes before "b", all good.
    {
        let d = Directory {
            directories: vec![
                DirectoryNode {
                    name: "a".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: 42,
                },
                DirectoryNode {
                    name: "b".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
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
                    name: "b".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: 42,
                },
                DirectoryNode {
                    name: "c".into(),
                    digest: DUMMY_DIGEST.to_vec().into(),
                    size: 42,
                },
            ],
            symlinks: vec![SymlinkNode {
                name: "a".into(),
                target: "foo".into(),
            }],
            ..Default::default()
        };

        d.validate().expect("validate shouldn't error");
    }
}

#[test]
fn validate_overflow() {
    let d = Directory {
        directories: vec![DirectoryNode {
            name: "foo".into(),
            digest: DUMMY_DIGEST.to_vec().into(),
            size: u64::MAX,
        }],
        ..Default::default()
    };

    match d.validate().expect_err("must fail") {
        ValidateDirectoryError::SizeOverflow => {}
        _ => panic!("unexpected error"),
    }
}

#[test]
fn add_nodes_to_directory() {
    let mut d = Directory {
        ..Default::default()
    };

    d.add(node::Node::Directory(DirectoryNode {
        name: "b".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
    }));
    d.add(node::Node::Directory(DirectoryNode {
        name: "a".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
    }));
    d.add(node::Node::Directory(DirectoryNode {
        name: "z".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
    }));

    d.add(node::Node::File(FileNode {
        name: "f".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
        executable: true,
    }));
    d.add(node::Node::File(FileNode {
        name: "c".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
        executable: true,
    }));
    d.add(node::Node::File(FileNode {
        name: "g".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
        executable: true,
    }));

    d.add(node::Node::Symlink(SymlinkNode {
        name: "t".into(),
        target: "a".into(),
    }));
    d.add(node::Node::Symlink(SymlinkNode {
        name: "o".into(),
        target: "a".into(),
    }));
    d.add(node::Node::Symlink(SymlinkNode {
        name: "e".into(),
        target: "a".into(),
    }));

    d.validate().expect("directory should be valid");
}

#[test]
#[cfg_attr(not(debug_assertions), ignore)]
#[should_panic = "name already exists in directories"]
fn add_duplicate_node_to_directory_panics() {
    let mut d = Directory {
        ..Default::default()
    };

    d.add(node::Node::Directory(DirectoryNode {
        name: "a".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
    }));
    d.add(node::Node::File(FileNode {
        name: "a".into(),
        digest: DUMMY_DIGEST.to_vec().into(),
        size: 1,
        executable: true,
    }));
}
