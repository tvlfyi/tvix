use crate::{
    directoryservice::{Directory, DirectoryNode, FileNode, Node, SymlinkNode},
    B3Digest,
};
use lazy_static::lazy_static;

pub const HELLOWORLD_BLOB_CONTENTS: &[u8] = b"Hello World!";
pub const EMPTY_BLOB_CONTENTS: &[u8] = b"";

lazy_static! {
    pub static ref DUMMY_DIGEST: B3Digest = {
        let u = [0u8; 32];
        (&u).into()
    };
    pub static ref DUMMY_DIGEST_2: B3Digest = {
        let mut u = [0u8; 32];
        u[0] = 0x10;
        (&u).into()
    };
    pub static ref DUMMY_DATA_1: bytes::Bytes = vec![0x01, 0x02, 0x03].into();
    pub static ref DUMMY_DATA_2: bytes::Bytes = vec![0x04, 0x05].into();

    pub static ref HELLOWORLD_BLOB_DIGEST: B3Digest =
        blake3::hash(HELLOWORLD_BLOB_CONTENTS).as_bytes().into();
    pub static ref EMPTY_BLOB_DIGEST: B3Digest =
        blake3::hash(EMPTY_BLOB_CONTENTS).as_bytes().into();

    // 2 bytes
    pub static ref BLOB_A: bytes::Bytes = vec![0x00, 0x01].into();
    pub static ref BLOB_A_DIGEST: B3Digest = blake3::hash(&BLOB_A).as_bytes().into();

    // 1MB
    pub static ref BLOB_B: bytes::Bytes = (0..255).collect::<Vec<u8>>().repeat(4 * 1024).into();
    pub static ref BLOB_B_DIGEST: B3Digest = blake3::hash(&BLOB_B).as_bytes().into();

    // Directories
    pub static ref DIRECTORY_WITH_KEEP: Directory = {
        let mut dir = Directory::new();
        dir.add(Node::File(FileNode::new(
            b".keep".to_vec().into(),
            EMPTY_BLOB_DIGEST.clone(),
            0,
            false
        ).unwrap())).unwrap();
        dir
    };
    pub static ref DIRECTORY_COMPLICATED: Directory = {
        let mut dir = Directory::new();
        dir.add(Node::Directory(DirectoryNode::new(
            b"keep".to_vec().into(),
            DIRECTORY_WITH_KEEP.digest(),
            DIRECTORY_WITH_KEEP.size()
        ).unwrap())).unwrap();
        dir.add(Node::File(FileNode::new(
            b".keep".to_vec().into(),
            EMPTY_BLOB_DIGEST.clone(),
            0,
            false
        ).unwrap())).unwrap();
        dir.add(Node::Symlink(SymlinkNode::new(
            b"aa".to_vec().into(),
            b"/nix/store/somewhereelse".to_vec().into()
        ).unwrap())).unwrap();
        dir
    };
    pub static ref DIRECTORY_A: Directory = Directory::new();
    pub static ref DIRECTORY_B: Directory = {
        let mut dir = Directory::new();
        dir.add(Node::Directory(DirectoryNode::new(
            b"a".to_vec().into(),
            DIRECTORY_A.digest(),
            DIRECTORY_A.size(),
        ).unwrap())).unwrap();
        dir
    };
    pub static ref DIRECTORY_C: Directory = {
        let mut dir = Directory::new();
        dir.add(Node::Directory(DirectoryNode::new(
            b"a".to_vec().into(),
            DIRECTORY_A.digest(),
            DIRECTORY_A.size(),
        ).unwrap())).unwrap();
        dir.add(Node::Directory(DirectoryNode::new(
                    b"a'".to_vec().into(),
                    DIRECTORY_A.digest(),
                    DIRECTORY_A.size(),
        ).unwrap())).unwrap();
        dir
    };
    pub static ref DIRECTORY_D: Directory = {
        let mut dir = Directory::new();
        dir.add(Node::Directory(DirectoryNode::new(
            b"a".to_vec().into(),
            DIRECTORY_A.digest(),
            DIRECTORY_A.size(),
        ).unwrap())).unwrap();
            dir.add(Node::Directory(DirectoryNode::new(
            b"b".to_vec().into(),
            DIRECTORY_B.digest(),
            DIRECTORY_B.size(),
        ).unwrap())).unwrap();
            dir

    };
}
