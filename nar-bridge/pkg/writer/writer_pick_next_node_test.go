package writer

import (
	"testing"

	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/google/go-cmp/cmp"
	"github.com/stretchr/testify/require"
	"google.golang.org/protobuf/testing/protocmp"
)

func requireProtoEq(t *testing.T, expected interface{}, actual interface{}) {
	if diff := cmp.Diff(expected, actual, protocmp.Transform()); diff != "" {
		t.Errorf("unexpected difference:\n%v", diff)
	}
}

func TestPopNextNode(t *testing.T) {
	t.Run("empty directory", func(t *testing.T) {
		d := &storev1pb.Directory{
			Directories: []*storev1pb.DirectoryNode{},
			Files:       []*storev1pb.FileNode{},
			Symlinks:    []*storev1pb.SymlinkNode{},
		}

		n := drainNextNode(d)
		require.Equal(t, nil, n)
	})
	t.Run("only directories", func(t *testing.T) {
		ds := &storev1pb.Directory{
			Directories: []*storev1pb.DirectoryNode{{
				Name:   []byte("a"),
				Digest: []byte{},
				Size:   0,
			}, {
				Name:   []byte("b"),
				Digest: []byte{},
				Size:   0,
			}},
			Files:    []*storev1pb.FileNode{},
			Symlinks: []*storev1pb.SymlinkNode{},
		}

		n := drainNextNode(ds)
		requireProtoEq(t, &storev1pb.DirectoryNode{
			Name:   []byte("a"),
			Digest: []byte{},
			Size:   0,
		}, n)
	})
}
