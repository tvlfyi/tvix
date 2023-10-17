package castorev1

// RenamedNode returns a node with a new name.
func RenamedNode(node *Node, name string) *Node {
	if directoryNode := node.GetDirectory(); directoryNode != nil {
		return &Node{
			Node: &Node_Directory{
				Directory: &DirectoryNode{
					Name:   []byte(name),
					Digest: directoryNode.GetDigest(),
					Size:   directoryNode.GetSize(),
				},
			},
		}
	} else if fileNode := node.GetFile(); fileNode != nil {
		return &Node{
			Node: &Node_File{
				File: &FileNode{
					Name:       []byte(name),
					Digest:     fileNode.GetDigest(),
					Size:       fileNode.GetSize(),
					Executable: fileNode.GetExecutable(),
				},
			},
		}
	} else if symlinkNode := node.GetSymlink(); symlinkNode != nil {
		return &Node{
			Node: &Node_Symlink{
				Symlink: &SymlinkNode{
					Name:   []byte(name),
					Target: symlinkNode.GetTarget(),
				},
			},
		}
	} else {
		panic("unreachable")
	}
}
