package storev1

import (
	"fmt"
	"io"
	"path"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"github.com/nix-community/go-nix/pkg/nar"
)

type DirectoryLookupFn func([]byte) (*castorev1pb.Directory, error)
type BlobLookupFn func([]byte) (io.ReadCloser, error)

// Export will traverse a given root node, and write the contents in NAR format
// to the passed Writer.
// It uses directoryLookupFn and blobLookupFn to resolve references.
func Export(
	w io.Writer,
	rootNode *castorev1pb.Node,
	directoryLookupFn DirectoryLookupFn,
	blobLookupFn BlobLookupFn,
) error {
	// initialize a NAR writer
	narWriter, err := nar.NewWriter(w)
	if err != nil {
		return fmt.Errorf("unable to initialize nar writer: %w", err)
	}
	defer narWriter.Close()

	// populate rootHeader
	rootHeader := &nar.Header{
		Path: "/",
	}

	// populate a stack
	// we will push paths and directories to it when entering a directory,
	// and emit individual elements to the NAR writer, draining the Directory object.
	// once it's empty, we can pop it off the stack.
	var stackPaths = []string{}
	var stackDirectories = []*castorev1pb.Directory{}

	// peek at the pathInfo root and assemble the root node and write to writer
	// in the case of a regular file, we retrieve and write the contents, close and exit
	// in the case of a symlink, we write the symlink, close and exit
	if fileNode := rootNode.GetFile(); fileNode != nil {
		rootHeader.Type = nar.TypeRegular
		rootHeader.Size = int64(fileNode.GetSize())
		rootHeader.Executable = fileNode.GetExecutable()
		err := narWriter.WriteHeader(rootHeader)
		if err != nil {
			return fmt.Errorf("unable to write root header: %w", err)
		}

		// if it's a regular file, retrieve and write the contents
		blobReader, err := blobLookupFn(fileNode.GetDigest())
		if err != nil {
			return fmt.Errorf("unable to lookup blob: %w", err)
		}
		defer blobReader.Close()

		_, err = io.Copy(narWriter, blobReader)
		if err != nil {
			return fmt.Errorf("unable to read from blobReader: %w", err)
		}

		err = blobReader.Close()
		if err != nil {
			return fmt.Errorf("unable to close content reader: %w", err)
		}

		err = narWriter.Close()
		if err != nil {
			return fmt.Errorf("unable to close nar reader: %w", err)
		}

		return nil
	} else if symlinkNode := rootNode.GetSymlink(); symlinkNode != nil {
		rootHeader.Type = nar.TypeSymlink
		rootHeader.LinkTarget = string(symlinkNode.GetTarget())
		err := narWriter.WriteHeader(rootHeader)
		if err != nil {
			return fmt.Errorf("unable to write root header: %w", err)
		}

		err = narWriter.Close()
		if err != nil {
			return fmt.Errorf("unable to close nar reader: %w", err)
		}
	} else if directoryNode := rootNode.GetDirectory(); directoryNode != nil {
		// We have a directory at the root, look it up and put in on the stack.
		directory, err := directoryLookupFn(directoryNode.GetDigest())
		if err != nil {
			return fmt.Errorf("unable to lookup directory: %w", err)
		}
		stackDirectories = append(stackDirectories, directory)
		stackPaths = append(stackPaths, "/")

		err = narWriter.WriteHeader(&nar.Header{
			Path: "/",
			Type: nar.TypeDirectory,
		})

		if err != nil {
			return fmt.Errorf("error writing header: %w", err)
		}
	} else {
		panic("invalid type") // unreachable
	}

	// as long as the stack is not empty, we keep running.
	for {
		if len(stackDirectories) == 0 {
			return nil
		}

		// Peek at the current top of the stack.
		topOfStack := stackDirectories[len(stackDirectories)-1]
		topOfStackPath := stackPaths[len(stackPaths)-1]

		// get the next element that's lexicographically smallest, and drain it from
		// the current directory on top of the stack.
		nextNode := drainNextNode(topOfStack)

		// If nextNode returns nil, there's nothing left in the directory node, so we
		// can emit it from the stack.
		// Contrary to the import case, we don't emit the node popping from the stack, but when pushing.
		if nextNode == nil {
			// pop off stack
			stackDirectories = stackDirectories[:len(stackDirectories)-1]
			stackPaths = stackPaths[:len(stackPaths)-1]

			continue
		}

		switch n := (nextNode).(type) {
		case *castorev1pb.DirectoryNode:
			err := narWriter.WriteHeader(&nar.Header{
				Path: path.Join(topOfStackPath, string(n.GetName())),
				Type: nar.TypeDirectory,
			})
			if err != nil {
				return fmt.Errorf("unable to write nar header: %w", err)
			}

			d, err := directoryLookupFn(n.GetDigest())
			if err != nil {
				return fmt.Errorf("unable to lookup directory: %w", err)
			}

			// add to stack
			stackDirectories = append(stackDirectories, d)
			stackPaths = append(stackPaths, path.Join(topOfStackPath, string(n.GetName())))
		case *castorev1pb.FileNode:
			err := narWriter.WriteHeader(&nar.Header{
				Path:       path.Join(topOfStackPath, string(n.GetName())),
				Type:       nar.TypeRegular,
				Size:       int64(n.GetSize()),
				Executable: n.GetExecutable(),
			})
			if err != nil {
				return fmt.Errorf("unable to write nar header: %w", err)
			}

			// copy file contents
			contentReader, err := blobLookupFn(n.GetDigest())
			if err != nil {
				return fmt.Errorf("unable to get blob: %w", err)
			}
			defer contentReader.Close()

			_, err = io.Copy(narWriter, contentReader)
			if err != nil {
				return fmt.Errorf("unable to copy contents from contentReader: %w", err)
			}

			err = contentReader.Close()
			if err != nil {
				return fmt.Errorf("unable to close content reader: %w", err)
			}
		case *castorev1pb.SymlinkNode:
			err := narWriter.WriteHeader(&nar.Header{
				Path:       path.Join(topOfStackPath, string(n.GetName())),
				Type:       nar.TypeSymlink,
				LinkTarget: string(n.GetTarget()),
			})
			if err != nil {
				return fmt.Errorf("unable to write nar header: %w", err)
			}
		}
	}
}

// drainNextNode will drain a directory message with one of its child nodes,
// whichever comes first alphabetically.
func drainNextNode(d *castorev1pb.Directory) interface{} {
	switch v := (smallestNode(d)).(type) {
	case *castorev1pb.DirectoryNode:
		d.Directories = d.Directories[1:]
		return v
	case *castorev1pb.FileNode:
		d.Files = d.Files[1:]
		return v
	case *castorev1pb.SymlinkNode:
		d.Symlinks = d.Symlinks[1:]
		return v
	case nil:
		return nil
	default:
		panic("invalid type encountered")
	}
}

// smallestNode will return the node from a directory message,
// whichever comes first alphabetically.
func smallestNode(d *castorev1pb.Directory) interface{} {
	childDirectories := d.GetDirectories()
	childFiles := d.GetFiles()
	childSymlinks := d.GetSymlinks()

	if len(childDirectories) > 0 {
		if len(childFiles) > 0 {
			if len(childSymlinks) > 0 {
				// directories,files,symlinks
				return smallerNode(smallerNode(childDirectories[0], childFiles[0]), childSymlinks[0])
			} else {
				// directories,files,!symlinks
				return smallerNode(childDirectories[0], childFiles[0])
			}
		} else {
			// directories,!files
			if len(childSymlinks) > 0 {
				// directories,!files,symlinks
				return smallerNode(childDirectories[0], childSymlinks[0])
			} else {
				// directories,!files,!symlinks
				return childDirectories[0]
			}
		}
	} else {
		// !directories
		if len(childFiles) > 0 {
			// !directories,files
			if len(childSymlinks) > 0 {
				// !directories,files,symlinks
				return smallerNode(childFiles[0], childSymlinks[0])
			} else {
				// !directories,files,!symlinks
				return childFiles[0]
			}
		} else {
			//!directories,!files
			if len(childSymlinks) > 0 {
				//!directories,!files,symlinks
				return childSymlinks[0]
			} else {
				//!directories,!files,!symlinks
				return nil
			}
		}
	}
}

// smallerNode compares two nodes by their name,
// and returns the one with the smaller name.
// both nodes may not be nil, we do check for these cases in smallestNode.
func smallerNode(a interface{ GetName() []byte }, b interface{ GetName() []byte }) interface{ GetName() []byte } {
	if string(a.GetName()) < string(b.GetName()) {
		return a
	} else {
		return b
	}
}
