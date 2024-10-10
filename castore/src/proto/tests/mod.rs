use super::{node, Node, SymlinkNode};

mod directory;

/// Create a node with an empty symlink target, and ensure it fails validation.
#[test]
fn convert_symlink_empty_target_invalid() {
    Node {
        node: Some(node::Node::Symlink(SymlinkNode {
            name: "foo".into(),
            target: "".into(),
        })),
    }
    .into_name_and_node()
    .expect_err("must fail validation");
}

/// Create a node with a symlink target including null bytes, and ensure it
/// fails validation.
#[test]
fn convert_symlink_target_null_byte_invalid() {
    Node {
        node: Some(node::Node::Symlink(SymlinkNode {
            name: "foo".into(),
            target: "foo\0".into(),
        })),
    }
    .into_name_and_node()
    .expect_err("must fail validation");
}
