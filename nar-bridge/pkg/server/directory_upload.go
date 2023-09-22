package server

import (
	"context"
	"encoding/base64"
	"fmt"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	log "github.com/sirupsen/logrus"
)

type DirectoriesUploader struct {
	ctx                       context.Context
	directoryServiceClient    castorev1pb.DirectoryServiceClient
	directoryServicePutStream castorev1pb.DirectoryService_PutClient
}

func NewDirectoriesUploader(ctx context.Context, directoryServiceClient castorev1pb.DirectoryServiceClient) *DirectoriesUploader {
	return &DirectoriesUploader{
		ctx:                       ctx,
		directoryServiceClient:    directoryServiceClient,
		directoryServicePutStream: nil,
	}
}

func (du *DirectoriesUploader) Put(directory *castorev1pb.Directory) error {
	directoryDgst, err := directory.Digest()
	if err != nil {
		return fmt.Errorf("failed calculating directory digest: %w", err)
	}

	// Send the directory to the directory service
	// If the stream hasn't been initialized yet, do it first
	if du.directoryServicePutStream == nil {
		directoryServicePutStream, err := du.directoryServiceClient.Put(du.ctx)
		if err != nil {
			return fmt.Errorf("unable to initialize directory service put stream: %v", err)
		}
		du.directoryServicePutStream = directoryServicePutStream
	}

	// send the directory out
	err = du.directoryServicePutStream.Send(directory)
	if err != nil {
		return fmt.Errorf("error sending directory: %w", err)
	}
	log.WithField("digest", base64.StdEncoding.EncodeToString(directoryDgst)).Debug("uploaded directory")

	return nil
}

// Done is called whenever we're
func (du *DirectoriesUploader) Done() (*castorev1pb.PutDirectoryResponse, error) {
	// only close once, and only if we opened.
	if du.directoryServicePutStream == nil {
		return nil, nil
	}
	putDirectoryResponse, err := du.directoryServicePutStream.CloseAndRecv()
	if err != nil {
		return nil, fmt.Errorf("unable to close directory service put stream: %v", err)
	}

	du.directoryServicePutStream = nil

	return putDirectoryResponse, nil
}
