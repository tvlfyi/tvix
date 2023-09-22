package writer

import (
	"testing"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
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
		d := &castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{},
			Files:       []*castorev1pb.FileNode{},
			Symlinks:    []*castorev1pb.SymlinkNode{},
		}

		n := drainNextNode(d)
		require.Equal(t, nil, n)
	})
	t.Run("only directories", func(t *testing.T) {
		ds := &castorev1pb.Directory{
			Directories: []*castorev1pb.DirectoryNode{{
				Name:   []byte("a"),
				Digest: []byte{},
				Size:   0,
			}, {
				Name:   []byte("b"),
				Digest: []byte{},
				Size:   0,
			}},
			Files:    []*castorev1pb.FileNode{},
			Symlinks: []*castorev1pb.SymlinkNode{},
		}

		n := drainNextNode(ds)
		requireProtoEq(t, &castorev1pb.DirectoryNode{
			Name:   []byte("a"),
			Digest: []byte{},
			Size:   0,
		}, n)
	})
}
