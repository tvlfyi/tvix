package reader

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"path"
	"strings"

	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/nix-community/go-nix/pkg/nar"
	"lukechampine.com/blake3"
)

type Reader struct {
	hrSha256 *Hasher
}

// An item on the directories stack
type item struct {
	path      string
	directory *storev1pb.Directory
}

func New(r io.Reader) *Reader {
	// Instead of using the underlying reader itself, wrap the reader
	// with a hasher calculating sha256 and one calculating sha512,
	// and feed that one into the NAR reader.
	hrSha256 := NewHasher(r, sha256.New())

	return &Reader{
		hrSha256: hrSha256,
	}
}

// Import reads from the internally-wrapped reader,
// and calls the callback functions whenever regular file contents are
// encountered, or a Directory node is about to be finished.
func (r *Reader) Import(
	ctx context.Context,
	// callback function called with each regular file content
	fileCb func(fileReader io.Reader) error,
	// callback function called with each finalized directory node
	directoryCb func(directory *storev1pb.Directory) error,
) (*storev1pb.PathInfo, error) {

	// construct a NAR reader, by reading through hrSha256
	narReader, err := nar.NewReader(r.hrSha256)
	if err != nil {
		return nil, fmt.Errorf("failed to instantiate nar reader: %w", err)
	}
	defer narReader.Close()

	// If we store a symlink or regular file at the root, these are not nil.
	// If they are nil, we instead have a stackDirectory.
	var rootSymlink *storev1pb.SymlinkNode
	var rootFile *storev1pb.FileNode
	var stackDirectory *storev1pb.Directory

	var stack = []item{}

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

		// if there's still a parent left on the stack, refer to it from there.
		if len(stack) > 0 {
			dgst, err := toPop.directory.Digest()
			if err != nil {
				return fmt.Errorf("unable to calculate directory digest: %w", err)
			}

			topOfStack := stack[len(stack)-1].directory
			topOfStack.Directories = append(topOfStack.Directories, &storev1pb.DirectoryNode{
				Name:   []byte(path.Base(toPop.path)),
				Digest: dgst,
				Size:   toPop.directory.Size(),
			})
		}
		// call the directoryCb
		if err := directoryCb(toPop.directory); err != nil {
			return fmt.Errorf("failed calling directoryCb: %w", err)
		}
		// Keep track that we have encounter at least one directory
		stackDirectory = toPop.directory
		return nil
	}

	// Assemble a PathInfo struct, the Node is populated later.
	assemblePathInfo := func() *storev1pb.PathInfo {
		return &storev1pb.PathInfo{
			Node:       nil,
			References: [][]byte{},
			Narinfo: &storev1pb.NARInfo{
				NarSize:        uint64(r.hrSha256.BytesWritten()),
				NarSha256:      r.hrSha256.Sum(nil),
				Signatures:     []*storev1pb.NARInfo_Signature{},
				ReferenceNames: []string{},
			},
		}
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
			return nil, ctx.Err()
		default:
			// call narReader.Next() to get the next element
			hdr, err := narReader.Next()

			// If this returns an error, it's either EOF (when we're done reading from the NAR),
			// or another error
			if err != nil {
				// if this returns no EOF, bail out
				if !errors.Is(err, io.EOF) {
					return nil, fmt.Errorf("failed getting next nar element: %w", err)
				}

				// The NAR has been read all the way to the end…
				// Make sure we close the nar reader, which might read some final trailers.
				if err := narReader.Close(); err != nil {
					return nil, fmt.Errorf("unable to close nar reader: %w", err)
				}

				// Check the stack. While it's not empty, we need to pop things off the stack.
				for len(stack) > 0 {
					err := popFromStack()
					if err != nil {
						return nil, fmt.Errorf("unable to pop from stack: %w", err)
					}

				}

				// Stack is empty. We now either have a regular or symlink root node, or we encountered at least one directory.
				// assemble pathInfo with these and return.
				pi := assemblePathInfo()
				if rootFile != nil {
					pi.Node = &storev1pb.Node{
						Node: &storev1pb.Node_File{
							File: rootFile,
						},
					}
				}
				if rootSymlink != nil {
					pi.Node = &storev1pb.Node{
						Node: &storev1pb.Node_Symlink{
							Symlink: rootSymlink,
						},
					}
				}
				if stackDirectory != nil {
					// calculate directory digest (i.e. after we received all its contents)
					dgst, err := stackDirectory.Digest()
					if err != nil {
						return nil, fmt.Errorf("unable to calculate root directory digest: %w", err)
					}

					pi.Node = &storev1pb.Node{
						Node: &storev1pb.Node_Directory{
							Directory: &storev1pb.DirectoryNode{
								Name:   []byte{},
								Digest: dgst,
								Size:   stackDirectory.Size(),
							},
						},
					}
				}
				return pi, nil
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
					return nil, fmt.Errorf("unable to pop from stack: %w", err)
				}
			}

			if hdr.Type == nar.TypeSymlink {
				symlinkNode := &storev1pb.SymlinkNode{
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
				// wrap reader with a reader calculating the blake3 hash
				fileReader := NewHasher(narReader, blake3.New(32, nil))

				err := fileCb(fileReader)
				if err != nil {
					return nil, fmt.Errorf("failure from fileCb: %w", err)
				}

				// drive the file reader to the end, in case the CB function doesn't read
				// all the way to the end on its own
				if fileReader.BytesWritten() != uint32(hdr.Size) {
					_, err := io.ReadAll(fileReader)
					if err != nil {
						return nil, fmt.Errorf("unable to read until the end of the file content: %w", err)
					}
				}

				// read the blake3 hash
				dgst := fileReader.Sum(nil)

				fileNode := &storev1pb.FileNode{
					Name:       []byte(getBasename(hdr.Path)),
					Digest:     dgst,
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
				directory := &storev1pb.Directory{
					Directories: []*storev1pb.DirectoryNode{},
					Files:       []*storev1pb.FileNode{},
					Symlinks:    []*storev1pb.SymlinkNode{},
				}
				stack = append(stack, item{
					directory: directory,
					path:      hdr.Path,
				})
			}
		}
	}
}
