package server

import (
	"bufio"
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"io"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	log "github.com/sirupsen/logrus"
)

// the size of individual BlobChunk we send when uploading to BlobService.
const chunkSize = 1024 * 1024

// this produces a callback function that can be used as blobCb for the
// importer.Import function call.
func genBlobServiceWriteCb(ctx context.Context, blobServiceClient castorev1pb.BlobServiceClient) func(io.Reader) error {
	return func(blobReader io.Reader) error {
		// Ensure the blobReader is buffered to at least the chunk size.
		blobReader = bufio.NewReaderSize(blobReader, chunkSize)

		putter, err := blobServiceClient.Put(ctx)
		if err != nil {
			// return error to the importer
			return fmt.Errorf("error from blob service: %w", err)
		}

		blobSize := 0
		chunk := make([]byte, chunkSize)

		for {
			n, err := blobReader.Read(chunk)
			if err != nil && !errors.Is(err, io.EOF) {
				return fmt.Errorf("unable to read from blobreader: %w", err)
			}

			if n != 0 {
				log.WithField("chunk_size", n).Debug("sending chunk")
				blobSize += n

				// send the blob chunk to the server. The err is only valid in the inner scope
				if err := putter.Send(&castorev1pb.BlobChunk{
					Data: chunk[:n],
				}); err != nil {
					return fmt.Errorf("sending blob chunk: %w", err)
				}
			}

			// if our read from blobReader returned an EOF, we're done reading
			if errors.Is(err, io.EOF) {
				break
			}

		}

		resp, err := putter.CloseAndRecv()
		if err != nil {
			return fmt.Errorf("close blob putter: %w", err)
		}

		log.WithFields(log.Fields{
			"blob_digest": base64.StdEncoding.EncodeToString(resp.GetDigest()),
			"blob_size":   blobSize,
		}).Debug("uploaded blob")

		return nil
	}
}
