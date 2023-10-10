package exporter_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"fmt"
	"io"
	"os"
	"testing"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"code.tvl.fyi/tvix/nar-bridge/pkg/exporter"
	"code.tvl.fyi/tvix/nar-bridge/pkg/importer"
	"github.com/stretchr/testify/require"
	"lukechampine.com/blake3"
)

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
	err = exporter.Export(
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
