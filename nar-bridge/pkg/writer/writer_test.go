package writer_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"fmt"
	"io"
	"os"
	"testing"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"code.tvl.fyi/tvix/nar-bridge/pkg/importer"
	"code.tvl.fyi/tvix/nar-bridge/pkg/writer"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/stretchr/testify/require"
	"lukechampine.com/blake3"
)

func mustDirectoryDigest(d *castorev1pb.Directory) []byte {
	dgst, err := d.Digest()
	if err != nil {
		panic(err)
	}
	return dgst
}

func mustBlobDigest(r io.Reader) []byte {
	hasher := blake3.New(32, nil)
	_, err := io.Copy(hasher, r)
	if err != nil {
		panic(err)
	}
	return hasher.Sum([]byte{})
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
	emptyDirectoryDigest := mustDirectoryDigest(emptyDirectory)

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

	blobsMap := make(map[string][]byte, 0)
	directoriesMap := make(map[string]*castorev1pb.Directory)

	pathInfo, err := importer.Import(
		context.Background(),
		bytes.NewBuffer(narContents),
		func(blobReader io.Reader) ([]byte, error) {
			// read in contents, we need to put it into filesMap later.
			contents, err := io.ReadAll(blobReader)
			require.NoError(t, err)

			dgst := mustBlobDigest(bytes.NewReader(contents))

			// put it in filesMap
			blobsMap[base64.StdEncoding.EncodeToString(dgst)] = contents

			return dgst, nil
		},
		func(directory *castorev1pb.Directory) ([]byte, error) {
			dgst := mustDirectoryDigest(directory)

			directoriesMap[base64.StdEncoding.EncodeToString(dgst)] = directory
			return dgst, nil
		},
	)

	require.NoError(t, err)

	// done populating everything, now actually test the export :-)
	var buf bytes.Buffer
	err = writer.Export(
		&buf,
		pathInfo,
		func(directoryDgst []byte) (*castorev1pb.Directory, error) {
			d, found := directoriesMap[base64.StdEncoding.EncodeToString(directoryDgst)]
			if !found {
				panic(fmt.Sprintf("directory %v not found", base64.StdEncoding.EncodeToString(directoryDgst)))
			}
			return d, nil
		},
		func(blobDgst []byte) (io.ReadCloser, error) {
			blobContents, found := blobsMap[base64.StdEncoding.EncodeToString(blobDgst)]
			if !found {
				panic(fmt.Sprintf("blob      %v not found", base64.StdEncoding.EncodeToString(blobDgst)))
			}
			return io.NopCloser(bytes.NewReader(blobContents)), nil
		},
	)

	require.NoError(t, err, "exporter shouldn't fail")
	require.Equal(t, narContents, buf.Bytes())
}
