package importer_test

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
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/stretchr/testify/require"
)

func TestRoundtrip(t *testing.T) {
	// We pipe nar_1094wph9z4nwlgvsd53abfz8i117ykiv5dwnq9nnhz846s7xqd7d.nar to
	// storev1pb.Export, and store all the file contents and directory objects
	// received in two hashmaps.
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
	err = storev1pb.Export(
		&buf,
		pathInfo.Node,
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
