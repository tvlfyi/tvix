use prost::Message;

tonic::include_proto!("tvix.store.v1");

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
}

#[cfg(test)]
mod tests {
    use super::{Directory, DirectoryNode, FileNode, SymlinkNode};

    #[test]
    fn test_directory_size() {
        {
            let d = Directory {
                directories: vec![],
                files: vec![],
                symlinks: vec![],
            };
            assert_eq!(d.size(), 0);
        }
        {
            let d = Directory {
                directories: vec![DirectoryNode {
                    name: String::from("foo"),
                    digest: vec![],
                    size: 0,
                }],
                files: vec![],
                symlinks: vec![],
            };
            assert_eq!(d.size(), 1);
        }
        {
            let d = Directory {
                directories: vec![DirectoryNode {
                    name: String::from("foo"),
                    digest: vec![],
                    size: 4,
                }],
                files: vec![],
                symlinks: vec![],
            };
            assert_eq!(d.size(), 5);
        }
        {
            let d = Directory {
                directories: vec![],
                files: vec![FileNode {
                    name: String::from("foo"),
                    digest: vec![],
                    size: 42,
                    executable: false,
                }],
                symlinks: vec![],
            };
            assert_eq!(d.size(), 1);
        }
        {
            let d = Directory {
                directories: vec![],
                files: vec![],
                symlinks: vec![SymlinkNode {
                    name: String::from("foo"),
                    target: String::from("bar"),
                }],
            };
            assert_eq!(d.size(), 1);
        }
    }

    #[test]
    fn test_digest() {
        let d = Directory {
            directories: vec![],
            files: vec![],
            symlinks: vec![],
        };

        assert_eq!(
            d.digest(),
            vec![
                0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
                0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
                0xe4, 0x1f, 0x32, 0x62
            ]
        )
    }
}
