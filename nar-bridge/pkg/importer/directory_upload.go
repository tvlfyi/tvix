package importer

import (
	"bytes"
	"context"
	"encoding/base64"
	"fmt"

	castorev1pb "code.tvl.fyi/tvix/castore-go"
	log "github.com/sirupsen/logrus"
)

// DirectoriesUploader opens a Put stream when it receives the first Put() call,
// and then uses the opened stream for subsequent Put() calls.
// When the uploading is finished, a call to Done() will close the stream and
// return the root digest returned from the directoryServiceClient.
type DirectoriesUploader struct {
	ctx                       context.Context
	directoryServiceClient    castorev1pb.DirectoryServiceClient
	directoryServicePutStream castorev1pb.DirectoryService_PutClient
	lastDirectoryDigest       []byte
}

func NewDirectoriesUploader(ctx context.Context, directoryServiceClient castorev1pb.DirectoryServiceClient) *DirectoriesUploader {
	return &DirectoriesUploader{
		ctx:                       ctx,
		directoryServiceClient:    directoryServiceClient,
		directoryServicePutStream: nil,
	}
}

func (du *DirectoriesUploader) Put(directory *castorev1pb.Directory) ([]byte, error) {
	directoryDigest, err := directory.Digest()
	if err != nil {
		return nil, fmt.Errorf("failed calculating directory digest: %w", err)
	}

	// Send the directory to the directory service
	// If the stream hasn't been initialized yet, do it first
	if du.directoryServicePutStream == nil {
		directoryServicePutStream, err := du.directoryServiceClient.Put(du.ctx)
		if err != nil {
			return nil, fmt.Errorf("unable to initialize directory service put stream: %v", err)
		}
		du.directoryServicePutStream = directoryServicePutStream
	}

	// send the directory out
	err = du.directoryServicePutStream.Send(directory)
	if err != nil {
		return nil, fmt.Errorf("error sending directory: %w", err)
	}
	log.WithField("digest", base64.StdEncoding.EncodeToString(directoryDigest)).Debug("uploaded directory")

	// update lastDirectoryDigest
	du.lastDirectoryDigest = directoryDigest

	return directoryDigest, nil
}

// Done closes the stream and returns the response.
// It returns null if closed for a second time.
func (du *DirectoriesUploader) Done() (*castorev1pb.PutDirectoryResponse, error) {
	// only close once, and only if we opened.
	if du.directoryServicePutStream == nil {
		return nil, nil
	}

	putDirectoryResponse, err := du.directoryServicePutStream.CloseAndRecv()
	if err != nil {
		return nil, fmt.Errorf("unable to close directory service put stream: %v", err)
	}

	// ensure the response contains the same digest as the one we have in lastDirectoryDigest.
	// Otherwise, the backend came up with another digest than we, in which we return an error.
	if !bytes.Equal(du.lastDirectoryDigest, putDirectoryResponse.RootDigest) {
		return nil, fmt.Errorf(
			"backend calculated different root digest as we, expected %s, actual %s",
			base64.StdEncoding.EncodeToString(du.lastDirectoryDigest),
			base64.StdEncoding.EncodeToString(putDirectoryResponse.RootDigest),
		)
	}

	// clear directoryServicePutStream.
	du.directoryServicePutStream = nil

	return putDirectoryResponse, nil
}
