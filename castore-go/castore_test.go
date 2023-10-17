package castorev1_test

import (
	"testing"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
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
		d := castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{},
			Files:       []*castorev1pb.FileNode{},
			Symlinks:    []*castorev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(0), d.Size())
	})

	t.Run("containing single empty directory", func(t *testing.T) {
		d := castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{{
				Name:   []byte([]byte("foo")),
				Digest: dummyDigest,
				Size:   0,
			}},
			Files:    []*castorev1pb.FileNode{},
			Symlinks: []*castorev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(1), d.Size())
	})

	t.Run("containing single non-empty directory", func(t *testing.T) {
		d := castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{{
				Name:   []byte("foo"),
				Digest: dummyDigest,
				Size:   4,
			}},
			Files:    []*castorev1pb.FileNode{},
			Symlinks: []*castorev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(5), d.Size())
	})

	t.Run("containing single file", func(t *testing.T) {
		d := castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{},
			Files: []*castorev1pb.FileNode{{
				Name:       []byte("foo"),
				Digest:     dummyDigest,
				Size:       42,
				Executable: false,
			}},
			Symlinks: []*castorev1pb.SymlinkNode{},
		}

		assert.Equal(t, uint32(1), d.Size())
	})

	t.Run("containing single symlink", func(t *testing.T) {
		d := castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{},
			Files:       []*castorev1pb.FileNode{},
			Symlinks: []*castorev1pb.SymlinkNode{{
				Name:   []byte("foo"),
				Target: []byte("bar"),
			}},
		}

		assert.Equal(t, uint32(1), d.Size())
	})

}
func TestDirectoryDigest(t *testing.T) {
	d := castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{},
		Files:       []*castorev1pb.FileNode{},
		Symlinks:    []*castorev1pb.SymlinkNode{},
	}

	dgst, err := d.Digest()
	assert.NoError(t, err, "calling Digest() on a directory shouldn't error")
	assert.Equal(t, []byte{
		0xaf, 0x13, 0x49, 0xb9, 0xf5, 0xf9, 0xa1, 0xa6, 0xa0, 0x40, 0x4d, 0xea, 0x36, 0xdc,
		0xc9, 0x49, 0x9b, 0xcb, 0x25, 0xc9, 0xad, 0xc1, 0x12, 0xb7, 0xcc, 0x9a, 0x93, 0xca,
		0xe4, 0x1f, 0x32, 0x62,
	}, dgst)
}

func TestDirectoryValidate(t *testing.T) {
	t.Run("empty", func(t *testing.T) {
		d := castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{},
			Files:       []*castorev1pb.FileNode{},
			Symlinks:    []*castorev1pb.SymlinkNode{},
		}

		assert.NoError(t, d.Validate())
	})

	t.Run("invalid names", func(t *testing.T) {
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{{
					Name:   []byte{},
					Digest: dummyDigest,
					Size:   42,
				}},
				Files:    []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{},
			}

			assert.ErrorContains(t, d.Validate(), "invalid node name")
		}
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{{
					Name:   []byte("."),
					Digest: dummyDigest,
					Size:   42,
				}},
				Files:    []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{},
			}

			assert.ErrorContains(t, d.Validate(), "invalid node name")
		}
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{},
				Files: []*castorev1pb.FileNode{{
					Name:       []byte(".."),
					Digest:     dummyDigest,
					Size:       42,
					Executable: false,
				}},
				Symlinks: []*castorev1pb.SymlinkNode{},
			}

			assert.ErrorContains(t, d.Validate(), "invalid node name")
		}
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{},
				Files:       []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{{
					Name:   []byte("\x00"),
					Target: []byte("foo"),
				}},
			}

			assert.ErrorContains(t, d.Validate(), "invalid node name")
		}
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{},
				Files:       []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{{
					Name:   []byte("foo/bar"),
					Target: []byte("foo"),
				}},
			}

			assert.ErrorContains(t, d.Validate(), "invalid node name")
		}
	})

	t.Run("invalid digest", func(t *testing.T) {
		d := castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{{
				Name:   []byte("foo"),
				Digest: nil,
				Size:   42,
			}},
			Files:    []*castorev1pb.FileNode{},
			Symlinks: []*castorev1pb.SymlinkNode{},
		}

		assert.ErrorContains(t, d.Validate(), "invalid digest length")
	})

	t.Run("invalid symlink targets", func(t *testing.T) {
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{},
				Files:       []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{{
					Name:   []byte("foo"),
					Target: []byte{},
				}},
			}

			assert.ErrorContains(t, d.Validate(), "invalid symlink target")
		}
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{},
				Files:       []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{{
					Name:   []byte("foo"),
					Target: []byte{0x66, 0x6f, 0x6f, 0},
				}},
			}

			assert.ErrorContains(t, d.Validate(), "invalid symlink target")
		}
	})

	t.Run("sorting", func(t *testing.T) {
		// "b" comes before "a", bad.
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{{
					Name:   []byte("b"),
					Digest: dummyDigest,
					Size:   42,
				}, {
					Name:   []byte("a"),
					Digest: dummyDigest,
					Size:   42,
				}},
				Files:    []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{},
			}
			assert.ErrorContains(t, d.Validate(), "is not in sorted order")
		}

		// "a" exists twice, bad.
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{{
					Name:   []byte("a"),
					Digest: dummyDigest,
					Size:   42,
				}},
				Files: []*castorev1pb.FileNode{{
					Name:       []byte("a"),
					Digest:     dummyDigest,
					Size:       42,
					Executable: false,
				}},
				Symlinks: []*castorev1pb.SymlinkNode{},
			}
			assert.ErrorContains(t, d.Validate(), "duplicate name")
		}

		// "a" comes before "b", all good.
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{{
					Name:   []byte("a"),
					Digest: dummyDigest,
					Size:   42,
				}, {
					Name:   []byte("b"),
					Digest: dummyDigest,
					Size:   42,
				}},
				Files:    []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{},
			}
			assert.NoError(t, d.Validate(), "shouldn't error")
		}

		// [b, c] and [a] are both properly sorted.
		{
			d := castorev1pb.Directory{
				Directories: []*castorev1pb.DirectoryNode{{
					Name:   []byte("b"),
					Digest: dummyDigest,
					Size:   42,
				}, {
					Name:   []byte("c"),
					Digest: dummyDigest,
					Size:   42,
				}},
				Files: []*castorev1pb.FileNode{},
				Symlinks: []*castorev1pb.SymlinkNode{{
					Name:   []byte("a"),
					Target: []byte("foo"),
				}},
			}
			assert.NoError(t, d.Validate(), "shouldn't error")
		}
	})
}
