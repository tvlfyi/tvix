package storev1_test

import (
	"path"
	"testing"

	"github.com/nix-community/go-nix/pkg/storepath"
	"github.com/stretchr/testify/assert"

	castorev1pb "code.tvl.fyi/tvix/castore-go"
	storev1pb "code.tvl.fyi/tvix/store-go"
)

const (
	EXAMPLE_STORE_PATH = "00bgd045z0d4icpbc2yyz4gx48ak44la-net-tools-1.60_p2017022118243"
)

var (
	exampleStorePathDigest = []byte{
		0x8a, 0x12, 0x32, 0x15, 0x22, 0xfd, 0x91, 0xef, 0xbd, 0x60, 0xeb, 0xb2, 0x48, 0x1a, 0xf8, 0x85,
		0x80, 0xf6, 0x16, 0x00}
)

func genPathInfoSymlink() *storev1pb.PathInfo {
	return &storev1pb.PathInfo{
		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_Symlink{
				Symlink: &castorev1pb.SymlinkNode{
					Name:   []byte("00000000000000000000000000000000-dummy"),
					Target: []byte("/nix/store/somewhereelse"),
				},
			},
		},
		References: [][]byte{exampleStorePathDigest},
		Narinfo: &storev1pb.NARInfo{
			NarSize:        0,
			NarSha256:      []byte{0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00},
			Signatures:     []*storev1pb.NARInfo_Signature{},
			ReferenceNames: []string{EXAMPLE_STORE_PATH},
		},
	}
}

func genPathInfoSymlinkThin() *storev1pb.PathInfo {
	pi := genPathInfoSymlink()
	pi.Narinfo = nil

	return pi
}

func TestValidate(t *testing.T) {
	t.Run("happy symlink", func(t *testing.T) {
		storePath, err := genPathInfoSymlink().Validate()
		assert.NoError(t, err, "PathInfo must validate")
		assert.Equal(t, "00000000000000000000000000000000-dummy", storePath.String())
	})

	t.Run("happy symlink thin", func(t *testing.T) {
		storePath, err := genPathInfoSymlinkThin().Validate()
		assert.NoError(t, err, "PathInfo must validate")
		assert.Equal(t, "00000000000000000000000000000000-dummy", storePath.String())
	})

	t.Run("invalid nar_sha256", func(t *testing.T) {
		pi := genPathInfoSymlink()

		// create broken references, where the reference digest is wrong
		pi.Narinfo.NarSha256 = []byte{0xbe, 0xef}

		_, err := pi.Validate()
		assert.Error(t, err, "must not validate")
	})

	t.Run("invalid reference digest", func(t *testing.T) {
		pi := genPathInfoSymlink()

		// create broken references, where the reference digest is wrong
		pi.References = append(pi.References, []byte{0x00})

		_, err := pi.Validate()
		assert.Error(t, err, "must not validate")
	})

	t.Run("invalid reference name", func(t *testing.T) {
		pi := genPathInfoSymlink()

		// make the reference name an invalid store path
		pi.Narinfo.ReferenceNames[0] = "00000000000000000000000000000000-"

		_, err := pi.Validate()
		assert.Error(t, err, "must not validate")
	})

	t.Run("reference name digest mismatch", func(t *testing.T) {
		pi := genPathInfoSymlink()

		// cause the digest for the reference to mismatch
		pi.Narinfo.ReferenceNames[0] = "11111111111111111111111111111111-dummy"

		_, err := pi.Validate()
		assert.Error(t, err, "must not validate")
	})

	t.Run("nil root node", func(t *testing.T) {
		pi := genPathInfoSymlink()

		pi.Node = nil

		_, err := pi.Validate()
		assert.Error(t, err, "must not validate")
	})

	t.Run("invalid root node name", func(t *testing.T) {
		pi := genPathInfoSymlink()

		// make the reference name an invalid store path - it may not be absolute
		symlinkNode := pi.Node.GetSymlink()
		symlinkNode.Name = []byte(path.Join(storepath.StoreDir, "00000000000000000000000000000000-dummy"))

		_, err := pi.Validate()
		assert.Error(t, err, "must not validate")
	})

	t.Run("happy deriver", func(t *testing.T) {
		pi := genPathInfoSymlink()

		// add the Deriver Field.
		pi.Narinfo.Deriver = &storev1pb.StorePath{
			Digest: exampleStorePathDigest,
			Name:   "foo",
		}

		_, err := pi.Validate()
		assert.NoError(t, err, "must validate")
	})

	t.Run("invalid deriver", func(t *testing.T) {
		pi := genPathInfoSymlink()

		// add the Deriver Field, with a broken digest
		pi.Narinfo.Deriver = &storev1pb.StorePath{
			Digest: []byte{},
			Name:   "foo2",
		}
		_, err := pi.Validate()
		assert.Error(t, err, "must not validate")
	})

}
