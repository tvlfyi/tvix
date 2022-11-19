package server

import (
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"context"
	"encoding/base64"
	"fmt"
	log "github.com/sirupsen/logrus"
	"io"
)

// this returns a callback function that can be used as fileCb
// for the reader.Import function call
func genBlobServiceWriteCb(ctx context.Context, blobServiceClient storev1pb.BlobServiceClient) func(io.Reader) error {
	return func(fileReader io.Reader) error {
		// Read from fileReader into a buffer.
		// We currently buffer all contents and send them to blobServiceClient at once,
		// but that's about to change.
		contents, err := io.ReadAll(fileReader)
		if err != nil {
			return fmt.Errorf("unable to read all contents from file reader: %w", err)
		}

		log := log.WithField("blob_size", len(contents))

		log.Infof("about to upload blob")

		putter, err := blobServiceClient.Put(ctx)
		if err != nil {
			// return error to the importer
			return fmt.Errorf("error from blob service: %w", err)
		}
		err = putter.Send(&storev1pb.BlobChunk{
			Data: contents,
		})
		if err != nil {
			return fmt.Errorf("putting blob chunk: %w", err)
		}
		resp, err := putter.CloseAndRecv()
		if err != nil {
			return fmt.Errorf("close blob putter: %w", err)
		}

		log.WithField("digest", base64.StdEncoding.EncodeToString(resp.GetDigest())).Info("uploaded blob")

		return nil
	}
}
