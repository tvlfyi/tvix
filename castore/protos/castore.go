package castorev1

import (
	"bytes"
	"encoding/base64"
	"fmt"
	"google.golang.org/protobuf/proto"
	"lukechampine.com/blake3"
)

// The size of a directory is calculated by summing up the numbers of
// `directories`, `files` and `symlinks`, and for each directory, its size
// field.
func (d *Directory) Size() uint32 {
	var size uint32
	size = uint32(len(d.Files) + len(d.Symlinks))
	for _, d := range d.Directories {
		size += 1 + d.Size
	}
	return size
}

func (d *Directory) Digest() ([]byte, error) {
	b, err := proto.MarshalOptions{
		Deterministic: true,
	}.Marshal(d)

	if err != nil {
		return nil, fmt.Errorf("error while marshalling directory: %w", err)
	}

	h := blake3.New(32, nil)

	_, err = h.Write(b)
	if err != nil {
		return nil, fmt.Errorf("error writing to hasher: %w", err)
	}

	return h.Sum(nil), nil
}

// isValidName checks a name for validity.
// We disallow slashes, null bytes, '.', '..' and the empty string.
// Depending on the context, a *Node message with an empty string as name is
// allowed, but they don't occur inside a Directory message.
func isValidName(n []byte) bool {
	if len(n) == 0 || bytes.Equal(n, []byte("..")) || bytes.Equal(n, []byte{'.'}) || bytes.Contains(n, []byte{'\x00'}) || bytes.Contains(n, []byte{'/'}) {
		return false
	}
	return true
}

// Validate thecks the Directory message for invalid data, such as:
// - violations of name restrictions
// - invalid digest lengths
// - not properly sorted lists
// - duplicate names in the three lists
func (d *Directory) Validate() error {
	// seenNames contains all seen names so far.
	// We populate this to ensure node names are unique across all three lists.
	seenNames := make(map[string]interface{})

	// We also track the last seen name in each of the three lists,
	// to ensure nodes are sorted by their names.
	var lastDirectoryName, lastFileName, lastSymlinkName []byte

	// helper function to only insert in sorted order.
	// used with the three lists above.
	// Note this consumes a *pointer to* a string,  as it mutates it.
	insertIfGt := func(lastName *[]byte, name []byte) error {
		// update if it's greater than the previous name
		if bytes.Compare(name, *lastName) == 1 {
			*lastName = name
			return nil
		} else {
			return fmt.Errorf("%v is not in sorted order", name)
		}
	}

	// insertOnce inserts into seenNames if the key doesn't exist yet.
	insertOnce := func(name []byte) error {
		encoded := base64.StdEncoding.EncodeToString(name)
		if _, found := seenNames[encoded]; found {
			return fmt.Errorf("duplicate name: %v", string(name))
		}
		seenNames[encoded] = nil
		return nil
	}

	// Loop over all Directories, Files and Symlinks individually.
	// Check the name for validity, check a potential digest for length,
	// then check for sorting in the current list, and uniqueness across all three lists.
	for _, directoryNode := range d.Directories {
		directoryName := directoryNode.GetName()

		// check name for validity
		if !isValidName(directoryName) {
			return fmt.Errorf("invalid name for DirectoryNode: %v", directoryName)
		}

		// check digest to be 32 bytes
		digestLen := len(directoryNode.GetDigest())
		if digestLen != 32 {
			return fmt.Errorf("invalid digest length for DirectoryNode: %d", digestLen)
		}

		// ensure names are sorted
		if err := insertIfGt(&lastDirectoryName, directoryName); err != nil {
			return err
		}

		// add to seenNames
		if err := insertOnce(directoryName); err != nil {
			return err
		}

	}

	for _, fileNode := range d.Files {
		fileName := fileNode.GetName()

		// check name for validity
		if !isValidName(fileName) {
			return fmt.Errorf("invalid name for FileNode: %v", fileName)
		}

		// check digest to be 32 bytes
		digestLen := len(fileNode.GetDigest())
		if digestLen != 32 {
			return fmt.Errorf("invalid digest length for FileNode: %d", digestLen)
		}

		// ensure names are sorted
		if err := insertIfGt(&lastFileName, fileName); err != nil {
			return err
		}

		// add to seenNames
		if err := insertOnce(fileName); err != nil {
			return err
		}
	}

	for _, symlinkNode := range d.Symlinks {
		symlinkName := symlinkNode.GetName()

		// check name for validity
		if !isValidName(symlinkName) {
			return fmt.Errorf("invalid name for SymlinkNode: %v", symlinkName)
		}

		// ensure names are sorted
		if err := insertIfGt(&lastSymlinkName, symlinkName); err != nil {
			return err
		}

		// add to seenNames
		if err := insertOnce(symlinkName); err != nil {
			return err
		}
	}

	return nil
}
