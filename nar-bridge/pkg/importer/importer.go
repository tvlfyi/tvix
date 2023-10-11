package importer

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"path"
	"strings"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"github.com/nix-community/go-nix/pkg/nar"
)

// An item on the directories stack
type stackItem struct {
	path      string
	directory *castorev1pb.Directory
}

// Import reads a NAR from a reader, and returns a the root node,
// NAR size and NAR sha256 digest.
func Import(
	// a context, to support cancellation
	ctx context.Context,
	// The reader the data is read from
	r io.Reader,
	// callback function called with each regular file content
	blobCb func(fileReader io.Reader) ([]byte, error),
	// callback function called with each finalized directory node
	directoryCb func(directory *castorev1pb.Directory) ([]byte, error),
) (*castorev1pb.Node, uint64, []byte, error) {
	// We need to wrap the underlying reader a bit.
	// - we want to keep track of the number of bytes read in total
	// - we calculate the sha256 digest over all data read
	// Express these two things in a MultiWriter, and give the NAR reader a
	// TeeReader that writes to it.
	narCountW := &CountingWriter{}
	sha256W := sha256.New()
	multiW := io.MultiWriter(narCountW, sha256W)
	narReader, err := nar.NewReader(io.TeeReader(r, multiW))
	if err != nil {
		return nil, 0, nil, fmt.Errorf("failed to instantiate nar reader: %w", err)
	}
	defer narReader.Close()

	// If we store a symlink or regular file at the root, these are not nil.
	// If they are nil, we instead have a stackDirectory.
	var rootSymlink *castorev1pb.SymlinkNode
	var rootFile *castorev1pb.FileNode
	var stackDirectory *castorev1pb.Directory

	var stack = []stackItem{}

	// popFromStack is used when we transition to a different directory or
	// drain the stack when we reach the end of the NAR.
	// It adds the popped element to the element underneath if any,
	// and passes it to the directoryCb callback.
	// This function may only be called if the stack is not already empty.
	popFromStack := func() error {
		// Keep the top item, and "resize" the stack slice.
		// This will only make the last element unaccessible, but chances are high
		// we're re-using that space anyways.
		toPop := stack[len(stack)-1]
		stack = stack[:len(stack)-1]

		// call the directoryCb
		directoryDigest, err := directoryCb(toPop.directory)
		if err != nil {
			return fmt.Errorf("failed calling directoryCb: %w", err)
		}

		// if there's still a parent left on the stack, refer to it from there.
		if len(stack) > 0 {
			topOfStack := stack[len(stack)-1].directory
			topOfStack.Directories = append(topOfStack.Directories, &castorev1pb.DirectoryNode{
				Name:   []byte(path.Base(toPop.path)),
				Digest: directoryDigest,
				Size:   toPop.directory.Size(),
			})
		}
		// Keep track that we have encounter at least one directory
		stackDirectory = toPop.directory
		return nil
	}

	getBasename := func(p string) string {
		// extract the basename. In case of "/", replace with empty string.
		basename := path.Base(p)
		if basename == "/" {
			basename = ""
		}
		return basename
	}

	for {
		select {
		case <-ctx.Done():
			return nil, 0, nil, ctx.Err()
		default:
			// call narReader.Next() to get the next element
			hdr, err := narReader.Next()

			// If this returns an error, it's either EOF (when we're done reading from the NAR),
			// or another error.
			if err != nil {
				// if this returns no EOF, bail out
				if !errors.Is(err, io.EOF) {
					return nil, 0, nil, fmt.Errorf("failed getting next nar element: %w", err)
				}

				// The NAR has been read all the way to the end…
				// Make sure we close the nar reader, which might read some final trailers.
				if err := narReader.Close(); err != nil {
					return nil, 0, nil, fmt.Errorf("unable to close nar reader: %w", err)
				}

				// Check the stack. While it's not empty, we need to pop things off the stack.
				for len(stack) > 0 {
					err := popFromStack()
					if err != nil {
						return nil, 0, nil, fmt.Errorf("unable to pop from stack: %w", err)
					}
				}

				// Stack is empty.
				// Now either root{File,Symlink,Directory} is not nil,
				// and we can return the root node.
				narSize := narCountW.BytesWritten()
				narSha256 := sha256W.Sum(nil)

				if rootFile != nil {
					return &castorev1pb.Node{
						Node: &castorev1pb.Node_File{
							File: rootFile,
						},
					}, narSize, narSha256, nil
				} else if rootSymlink != nil {
					return &castorev1pb.Node{
						Node: &castorev1pb.Node_Symlink{
							Symlink: rootSymlink,
						},
					}, narSize, narSha256, nil
				} else if stackDirectory != nil {
					// calculate directory digest (i.e. after we received all its contents)
					dgst, err := stackDirectory.Digest()
					if err != nil {
						return nil, 0, nil, fmt.Errorf("unable to calculate root directory digest: %w", err)
					}

					return &castorev1pb.Node{
						Node: &castorev1pb.Node_Directory{
							Directory: &castorev1pb.DirectoryNode{
								Name:   []byte{},
								Digest: dgst,
								Size:   stackDirectory.Size(),
							},
						},
					}, narSize, narSha256, nil
				} else {
					return nil, 0, nil, fmt.Errorf("no root set")
				}
			}

			// Check for valid path transitions, pop from stack if needed
			// The nar reader already gives us some guarantees about ordering and illegal transitions,
			// So we really only need to check if the top-of-stack path is a prefix of the path,
			// and if it's not, pop from the stack. We do this repeatedly until the top of the stack is
			// the subdirectory the new entry is in, or we hit the root directory.

			// We don't need to worry about the root node case, because we can only finish the root "/"
			// If we're at the end of the NAR reader (covered by the EOF check)
			for len(stack) > 1 && !strings.HasPrefix(hdr.Path, stack[len(stack)-1].path+"/") {
				err := popFromStack()
				if err != nil {
					return nil, 0, nil, fmt.Errorf("unable to pop from stack: %w", err)
				}
			}

			if hdr.Type == nar.TypeSymlink {
				symlinkNode := &castorev1pb.SymlinkNode{
					Name:   []byte(getBasename(hdr.Path)),
					Target: []byte(hdr.LinkTarget),
				}
				if len(stack) > 0 {
					topOfStack := stack[len(stack)-1].directory
					topOfStack.Symlinks = append(topOfStack.Symlinks, symlinkNode)
				} else {
					rootSymlink = symlinkNode
				}

			}
			if hdr.Type == nar.TypeRegular {
				// wrap reader with a reader counting the number of bytes read
				blobCountW := &CountingWriter{}
				blobReader := io.TeeReader(narReader, blobCountW)

				blobDigest, err := blobCb(blobReader)
				if err != nil {
					return nil, 0, nil, fmt.Errorf("failure from blobCb: %w", err)
				}

				// ensure blobCb did read all the way to the end.
				// If it didn't, the blobCb function is wrong and we should bail out.
				if blobCountW.BytesWritten() != uint64(hdr.Size) {
					panic("blobCB did not read to end")
				}

				fileNode := &castorev1pb.FileNode{
					Name:       []byte(getBasename(hdr.Path)),
					Digest:     blobDigest,
					Size:       uint32(hdr.Size),
					Executable: hdr.Executable,
				}
				if len(stack) > 0 {
					topOfStack := stack[len(stack)-1].directory
					topOfStack.Files = append(topOfStack.Files, fileNode)
				} else {
					rootFile = fileNode
				}
			}
			if hdr.Type == nar.TypeDirectory {
				directory := &castorev1pb.Directory{
					Directories: []*castorev1pb.DirectoryNode{},
					Files:       []*castorev1pb.FileNode{},
					Symlinks:    []*castorev1pb.SymlinkNode{},
				}
				stack = append(stack, stackItem{
					directory: directory,
					path:      hdr.Path,
				})
			}
		}
	}
}
