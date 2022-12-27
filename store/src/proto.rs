use anyhow::Result;
use std::collections::HashSet;
use thiserror::Error;

use prost::Message;

tonic::include_proto!("tvix.store.v1");

/// Errors that can occur during the validation of Directory messages.
#[derive(Debug, Error, PartialEq)]
pub enum ValidateDirectoryError {
    /// Elements are not in sorted order
    #[error("{0} is not sorted")]
    WrongSorting(String),
    /// Multiple elements with the same name encountered
    #[error("{0} is a duplicate name")]
    DuplicateName(String),
    /// Invalid name encountered
    #[error("Invalid name in {0}")]
    InvalidName(String),
    /// Invalid digest length encountered
    #[error("Ivalid Digest length: {0}")]
    InvalidDigestLen(usize),
}

/// Checks a name for validity.
/// We disallow slashes, null bytes, '.', '..' and the empty string.
/// Depending on the context, a [DirectoryNode], [FileNode] or [SymlinkNode]
/// message with an empty string as name is allowed, but they don't occur
/// inside a Directory message.
fn validate_node_name(name: &str) -> Result<(), ValidateDirectoryError> {
    if name == "" || name == ".." || name == "." || name.contains("\x00") || name.contains("/") {
        return Err(ValidateDirectoryError::InvalidName(
            name.to_string().clone(),
        ));
    }
    Ok(())
}

/// Checks a digest for validity.
/// Digests are 32 bytes long, as we store blake3 digests.
fn validate_digest(digest: &Vec<u8>) -> Result<(), ValidateDirectoryError> {
    if digest.len() != 32 {
        return Err(ValidateDirectoryError::InvalidDigestLen(digest.len()));
    }
    Ok(())
}

/// Accepts a name, and a mutable reference to the previous name.
/// If the passed name is larger than the previous one, the reference is updated.
/// If it's not, an error is returned.
fn update_if_lt_prev<'set, 'n>(
    prev_name: &'set mut &'n str,
    name: &'n str,
) -> Result<(), ValidateDirectoryError> {
    if *name < **prev_name {
        return Err(ValidateDirectoryError::WrongSorting(
            name.to_string().clone(),
        ));
    }
    *prev_name = name;
    Ok(())
}

/// Inserts the given name into a HashSet if it's not already in there.
/// If it is, an error is returned.
fn insert_once<'n>(
    seen_names: &mut HashSet<&'n str>,
    name: &'n str,
) -> Result<(), ValidateDirectoryError> {
    if seen_names.get(name).is_some() {
        return Err(ValidateDirectoryError::DuplicateName(
            name.to_string().clone(),
        ));
    }
    seen_names.insert(name);
    Ok(())
}

impl Directory {
    // The size of a directory is the number of all regular and symlink elements,
    // the number of directory elements, and their size fields.
    pub fn size(&self) -> u32 {
        self.files.len() as u32
            + self.symlinks.len() as u32
            + self
                .directories
                .iter()
                .fold(0, |acc: u32, e| (acc + 1 + e.size) as u32)
    }

    pub fn digest(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();

        hasher.update(&self.encode_to_vec()).finalize().as_bytes()[..].to_vec()
    }

    /// validate checks the directory for invalid data, such as:
    /// - violations of name restrictions
    /// - invalid digest lengths
    /// - not properly sorted lists
    /// - duplicate names in the three lists
    pub fn validate(&self) -> Result<(), ValidateDirectoryError> {
        let mut seen_names: HashSet<&str> = HashSet::new();

        let mut last_directory_name: &str = "";
        let mut last_file_name: &str = "";
        let mut last_symlink_name: &str = "";

        // check directories
        for directory_node in &self.directories {
            validate_node_name(&directory_node.name)?;
            validate_digest(&directory_node.digest)?;

            update_if_lt_prev(&mut last_directory_name, &mut directory_node.name.as_str())?;
            insert_once(&mut seen_names, &directory_node.name.as_str())?;
        }

        // check files
        for file_node in &self.files {
            validate_node_name(&file_node.name)?;
            validate_digest(&file_node.digest)?;

            update_if_lt_prev(&mut last_file_name, &mut file_node.name.as_str())?;
            insert_once(&mut seen_names, &file_node.name.as_str())?;
        }

        // check symlinks
        for symlink_node in &self.symlinks {
            validate_node_name(&symlink_node.name)?;

            update_if_lt_prev(&mut last_symlink_name, &mut symlink_node.name.as_str())?;
            insert_once(&mut seen_names, &symlink_node.name.as_str())?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Directory, DirectoryNode, FileNode, SymlinkNode, ValidateDirectoryError};
    use lazy_static::lazy_static;

    lazy_static! {
        static ref DUMMY_DIGEST: Vec<u8> = vec![
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00,
        ];
    }
    #[test]
    fn test_directory_size() {
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
    fn test_digest() {
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
    fn test_directory_validate_empty() {
        let d = Directory::default();
        assert_eq!(d.validate(), Ok(()));
    }

    #[test]
    fn test_directory_validate_invalid_names() {
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
    fn test_directory_validate_invalid_digest() {
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
    fn test_directory_validate_sorting() {
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
}
