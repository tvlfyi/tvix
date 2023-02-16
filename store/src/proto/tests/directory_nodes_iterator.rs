use crate::proto::node::Node;
use crate::proto::Directory;
use crate::proto::DirectoryNode;
use crate::proto::FileNode;
use crate::proto::SymlinkNode;

#[test]
fn iterator() {
    let d = Directory {
        directories: vec![
            DirectoryNode {
                name: "c".to_string(),
                ..DirectoryNode::default()
            },
            DirectoryNode {
                name: "d".to_string(),
                ..DirectoryNode::default()
            },
            DirectoryNode {
                name: "h".to_string(),
                ..DirectoryNode::default()
            },
            DirectoryNode {
                name: "l".to_string(),
                ..DirectoryNode::default()
            },
        ],
        files: vec![
            FileNode {
                name: "b".to_string(),
                ..FileNode::default()
            },
            FileNode {
                name: "e".to_string(),
                ..FileNode::default()
            },
            FileNode {
                name: "g".to_string(),
                ..FileNode::default()
            },
            FileNode {
                name: "j".to_string(),
                ..FileNode::default()
            },
        ],
        symlinks: vec![
            SymlinkNode {
                name: "a".to_string(),
                ..SymlinkNode::default()
            },
            SymlinkNode {
                name: "f".to_string(),
                ..SymlinkNode::default()
            },
            SymlinkNode {
                name: "i".to_string(),
                ..SymlinkNode::default()
            },
            SymlinkNode {
                name: "k".to_string(),
                ..SymlinkNode::default()
            },
        ],
    };

    let mut node_names: Vec<String> = vec![];

    for node in d.nodes() {
        match node {
            Node::Directory(n) => node_names.push(n.name),
            Node::File(n) => node_names.push(n.name),
            Node::Symlink(n) => node_names.push(n.name),
        };
    }

    assert_eq!(
        vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"],
        node_names
    );
}
