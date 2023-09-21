use crate::proto::Directory;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::NamedNode;
use crate::proto::SymlinkNode;

#[test]
fn iterator() {
    let d = Directory {
        directories: vec![
            DirectoryNode {
                name: "c".into(),
                ..DirectoryNode::default()
            },
            DirectoryNode {
                name: "d".into(),
                ..DirectoryNode::default()
            },
            DirectoryNode {
                name: "h".into(),
                ..DirectoryNode::default()
            },
            DirectoryNode {
                name: "l".into(),
                ..DirectoryNode::default()
            },
        ],
        files: vec![
            FileNode {
                name: "b".into(),
                ..FileNode::default()
            },
            FileNode {
                name: "e".into(),
                ..FileNode::default()
            },
            FileNode {
                name: "g".into(),
                ..FileNode::default()
            },
            FileNode {
                name: "j".into(),
                ..FileNode::default()
            },
        ],
        symlinks: vec![
            SymlinkNode {
                name: "a".into(),
                ..SymlinkNode::default()
            },
            SymlinkNode {
                name: "f".into(),
                ..SymlinkNode::default()
            },
            SymlinkNode {
                name: "i".into(),
                ..SymlinkNode::default()
            },
            SymlinkNode {
                name: "k".into(),
                ..SymlinkNode::default()
            },
        ],
    };

    // We keep this strings here and convert to string to make the comparison
    // less messy.
    let mut node_names: Vec<String> = vec![];

    for node in d.nodes() {
        node_names.push(String::from_utf8(node.get_name().to_vec()).unwrap());
    }

    assert_eq!(
        vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"],
        node_names
    );
}
