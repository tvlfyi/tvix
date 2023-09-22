package writer_test

import (
	"bytes"
	"context"
	"encoding/hex"
	"io"
	"os"
	"testing"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"code.tvl.fyi/tvix/nar-bridge/pkg/reader"
	"code.tvl.fyi/tvix/nar-bridge/pkg/writer"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/stretchr/testify/require"
	"lukechampine.com/blake3"
)

func mustDigest(d *castorev1pb.Directory) []byte {
	dgst, err := d.Digest()
	if err != nil {
		panic(err)
	}
	return dgst
}

func TestSymlink(t *testing.T) {
	pathInfo := &storev1pb.PathInfo{

		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_Symlink{
				Symlink: &castorev1pb.SymlinkNode{
					Name:   []byte("doesntmatter"),
					Target: []byte("/nix/store/somewhereelse"),
				},
			},
		},
	}

	var buf bytes.Buffer

	err := writer.Export(&buf, pathInfo, func([]byte) (*castorev1pb.Directory, error) {
		panic("no directories expected")
	}, func([]byte) (io.ReadCloser, error) {
		panic("no files expected")
	})
	require.NoError(t, err, "exporter shouldn't fail")

	f, err := os.Open("../../testdata/symlink.nar")
	require.NoError(t, err)

	bytesExpected, err := io.ReadAll(f)
	if err != nil {
		panic(err)
	}

	require.Equal(t, bytesExpected, buf.Bytes(), "expected nar contents to match")
}

func TestRegular(t *testing.T) {
	// The blake3 digest of the 0x01 byte.
	BLAKE3_DIGEST_0X01 := []byte{
		0x48, 0xfc, 0x72, 0x1f, 0xbb, 0xc1, 0x72, 0xe0, 0x92, 0x5f, 0xa2, 0x7a, 0xf1, 0x67, 0x1d,
		0xe2, 0x25, 0xba, 0x92, 0x71, 0x34, 0x80, 0x29, 0x98, 0xb1, 0x0a, 0x15, 0x68, 0xa1, 0x88,
		0x65, 0x2b,
	}

	pathInfo := &storev1pb.PathInfo{
		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_File{
				File: &castorev1pb.FileNode{
					Name:       []byte("doesntmatter"),
					Digest:     BLAKE3_DIGEST_0X01,
					Size:       1,
					Executable: false,
				},
			},
		},
	}

	var buf bytes.Buffer

	err := writer.Export(&buf, pathInfo, func([]byte) (*castorev1pb.Directory, error) {
		panic("no directories expected")
	}, func(blobRef []byte) (io.ReadCloser, error) {
		if !bytes.Equal(blobRef, BLAKE3_DIGEST_0X01) {
			panic("unexpected blobref")
		}
		return io.NopCloser(bytes.NewBuffer([]byte{0x01})), nil
	})
	require.NoError(t, err, "exporter shouldn't fail")

	f, err := os.Open("../../testdata/onebyteregular.nar")
	require.NoError(t, err)

	bytesExpected, err := io.ReadAll(f)
	if err != nil {
		panic(err)
	}

	require.Equal(t, bytesExpected, buf.Bytes(), "expected nar contents to match")
}

func TestEmptyDirectory(t *testing.T) {
	// construct empty directory node this refers to
	emptyDirectory := &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{},
		Files:       []*castorev1pb.FileNode{},
		Symlinks:    []*castorev1pb.SymlinkNode{},
	}
	emptyDirectoryDigest := mustDigest(emptyDirectory)

	pathInfo := &storev1pb.PathInfo{
		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_Directory{
				Directory: &castorev1pb.DirectoryNode{
					Name:   []byte("doesntmatter"),
					Digest: emptyDirectoryDigest,
					Size:   0,
				},
			},
		},
	}

	var buf bytes.Buffer

	err := writer.Export(&buf, pathInfo, func(directoryRef []byte) (*castorev1pb.Directory, error) {
		if !bytes.Equal(directoryRef, emptyDirectoryDigest) {
			panic("unexpected directoryRef")
		}
		return emptyDirectory, nil
	}, func([]byte) (io.ReadCloser, error) {
		panic("no files expected")
	})
	require.NoError(t, err, "exporter shouldn't fail")

	f, err := os.Open("../../testdata/emptydirectory.nar")
	require.NoError(t, err)

	bytesExpected, err := io.ReadAll(f)
	if err != nil {
		panic(err)
	}

	require.Equal(t, bytesExpected, buf.Bytes(), "expected nar contents to match")
}

func TestFull(t *testing.T) {
	// We pipe nar_1094wph9z4nwlgvsd53abfz8i117ykiv5dwnq9nnhz846s7xqd7d.nar to the exporter,
	// and store all the file contents and directory objects received in two hashmaps.
	// We then feed it to the writer, and test we come up with the same NAR file.

	f, err := os.Open("../../testdata/nar_1094wph9z4nwlgvsd53abfz8i117ykiv5dwnq9nnhz846s7xqd7d.nar")
	require.NoError(t, err)

	narContents, err := io.ReadAll(f)
	require.NoError(t, err)

	filesMap := make(map[string][]byte, 0)
	directoriesMap := make(map[string]*castorev1pb.Directory)

	r := reader.New(bytes.NewBuffer(narContents))
	pathInfo, err := r.Import(
		context.Background(),
		func(fileReader io.Reader) error {
			fileContents, err := io.ReadAll(fileReader)
			require.NoError(t, err)

			b3Writer := blake3.New(32, nil)
			_, err = io.Copy(b3Writer, bytes.NewReader(fileContents))
			require.NoError(t, err)

			// put it in filesMap
			filesMap[hex.EncodeToString(b3Writer.Sum(nil))] = fileContents

			return nil
		},
		func(directory *castorev1pb.Directory) error {
			dgst := mustDigest(directory)

			directoriesMap[hex.EncodeToString(dgst)] = directory
			return nil
		},
	)

	require.NoError(t, err)

	// done populating everything, now actually test the export :-)

	var buf bytes.Buffer
	err = writer.Export(
		&buf,
		pathInfo,
		func(directoryRef []byte) (*castorev1pb.Directory, error) {
			d, found := directoriesMap[hex.EncodeToString(directoryRef)]
			if !found {
				panic("directories not found")
			}
			return d, nil
		},
		func(fileRef []byte) (io.ReadCloser, error) {
			fileContents, found := filesMap[hex.EncodeToString(fileRef)]
			if !found {
				panic("file not found")
			}
			return io.NopCloser(bytes.NewReader(fileContents)), nil
		},
	)

	require.NoError(t, err, "exporter shouldn't fail")
	require.Equal(t, narContents, buf.Bytes())
}
