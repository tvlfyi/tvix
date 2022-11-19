package storev1_test

import (
	"testing"

	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/stretchr/testify/assert"
)

var (
	dummyDigest = []byte{
		0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
		0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
		0x00, 0x00, 0x00, 0x00,
	}
)

func TestDirectorySize(t *testing.T) {
	t.Run("empty", func(t *testing.T) {
		d := storev1pb.Directory{
			Directories: []*storev1pb.DirectoryNode{},
			Files:       []*storev1pb.FileNode{},
			Symlinks:    []*storev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(0), d.Size())
	})

	t.Run("containing single empty directory", func(t *testing.T) {
		d := storev1pb.Directory{
			Directories: []*storev1pb.DirectoryNode{{
				Name:   "foo",
				Digest: dummyDigest,
				Size:   0,
			}},
			Files:    []*storev1pb.FileNode{},
			Symlinks: []*storev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(1), d.Size())
	})

	t.Run("containing single non-empty directory", func(t *testing.T) {
		d := storev1pb.Directory{
			Directories: []*storev1pb.DirectoryNode{{
				Name:   "foo",
				Digest: dummyDigest,
				Size:   4,
			}},
			Files:    []*storev1pb.FileNode{},
			Symlinks: []*storev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(5), d.Size())
	})

	t.Run("containing single file", func(t *testing.T) {
		d := storev1pb.Directory{
			Directories: []*storev1pb.DirectoryNode{},
			Files: []*storev1pb.FileNode{{
				Name:       "foo",
				Digest:     dummyDigest,
				Size:       42,
				Executable: false,
			}},
			Symlinks: []*storev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(1), d.Size())
	})

	t.Run("containing single symlink", func(t *testing.T) {
		d := storev1pb.Directory{
			Directories: []*storev1pb.DirectoryNode{},
			Files:       []*storev1pb.FileNode{},
			Symlinks: []*storev1pb.SymlinkNode{{
				Name:   "foo",
				Target: "bar",
			}},
		}

		assert.Equal(t, uint32(1), d.Size())
	})

}
func TestDirectoryDigest(t *testing.T) {
	d := storev1pb.Directory{
		Directories: []*storev1pb.DirectoryNode{},
		Files:       []*storev1pb.FileNode{},
		Symlinks:    []*storev1pb.SymlinkNode{},
	}

	dgst, err := d.Digest()
	assert.NoError(t, err, "calling Digest() on a directory shouldn't error")
	assert.Equal(t, []byte{
		0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
		0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
		0xe4, 0x1f, 0x32, 0x62,
	}, dgst)
}
