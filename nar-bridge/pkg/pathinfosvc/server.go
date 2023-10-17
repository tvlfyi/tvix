package pathinfosvc

import (
	"bufio"
	"bytes"
	"context"
	"encoding/base64"
	"fmt"
	"io"
	"net/http"
	"net/url"

	castorev1pb "code.tvl.fyi/tvix/castore-go"
	"code.tvl.fyi/tvix/nar-bridge/pkg/importer"
	storev1pb "code.tvl.fyi/tvix/store-go"
	mh "github.com/multiformats/go-multihash/core"
	"github.com/nix-community/go-nix/pkg/narinfo"
	"github.com/nix-community/go-nix/pkg/nixbase32"
	"github.com/sirupsen/logrus"
	"github.com/ulikunitz/xz"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

var _ storev1pb.PathInfoServiceServer = &PathInfoServiceServer{}

// PathInfoServiceServer exposes a Nix HTTP Binary Cache as a storev1pb.PathInfoServiceServer.
type PathInfoServiceServer struct {
	storev1pb.UnimplementedPathInfoServiceServer
	httpEndpoint *url.URL
	httpClient   *http.Client
	// TODO: signatures

	directoryServiceClient castorev1pb.DirectoryServiceClient
	blobServiceClient      castorev1pb.BlobServiceClient
}

func New(httpEndpoint *url.URL, httpClient *http.Client, directoryServiceClient castorev1pb.DirectoryServiceClient, blobServiceClient castorev1pb.BlobServiceClient) *PathInfoServiceServer {
	return &PathInfoServiceServer{
		httpEndpoint:           httpEndpoint,
		httpClient:             httpClient,
		directoryServiceClient: directoryServiceClient,
		blobServiceClient:      blobServiceClient,
	}
}

// CalculateNAR implements storev1.PathInfoServiceServer.
// It returns PermissionDenied, as clients are supposed to calculate NAR hashes themselves.
func (*PathInfoServiceServer) CalculateNAR(context.Context, *castorev1pb.Node) (*storev1pb.CalculateNARResponse, error) {
	return nil, status.Error(codes.PermissionDenied, "do it yourself please")
}

// Get implements storev1.PathInfoServiceServer.
// It only supports lookup my outhash, translates them to a corresponding GET $outhash.narinfo request,
// ingests the NAR file, while populating blob and directory service, then returns the PathInfo node.
// Subsequent requests will traverse the NAR file again, so make sure to compose this with another
// PathInfoService as caching layer.
func (p *PathInfoServiceServer) Get(ctx context.Context, getPathInfoRequest *storev1pb.GetPathInfoRequest) (*storev1pb.PathInfo, error) {
	outputHash := getPathInfoRequest.GetByOutputHash()
	if outputHash == nil {
		return nil, status.Error(codes.Unimplemented, "only by output hash supported")
	}

	// construct NARInfo URL
	narinfoURL := p.httpEndpoint.JoinPath(fmt.Sprintf("%v.narinfo", nixbase32.EncodeToString(outputHash)))

	log := logrus.WithField("output_hash", base64.StdEncoding.EncodeToString(outputHash))

	// We start right with a GET request, rather than doing a HEAD request.
	// If a request to the PathInfoService reaches us, an upper layer *wants* it
	// from us.
	// Doing a HEAD first wouldn't give us anything, we can still react on the Not
	// Found situation when doing the GET request.
	niRq, err := http.NewRequestWithContext(ctx, "GET", narinfoURL.String(), nil)
	if err != nil {
		log.WithError(err).Error("unable to construct NARInfo request")
		return nil, status.Errorf(codes.Internal, "unable to construct NARInfo request")
	}

	// Do the actual request; this follows redirects.
	niResp, err := p.httpClient.Do(niRq)
	if err != nil {
		log.WithError(err).Error("unable to do NARInfo request")
		return nil, status.Errorf(codes.Internal, "unable to do NARInfo request")
	}
	defer niResp.Body.Close()

	// In the case of a 404, return a NotFound.
	// We also return a NotFound in case of a 403 - this is to match the behaviour as Nix,
	// when querying nix-cache.s3.amazonaws.com directly, rather than cache.nixos.org.
	if niResp.StatusCode == http.StatusNotFound || niResp.StatusCode == http.StatusForbidden {
		log.Warn("no NARInfo found")
		return nil, status.Error(codes.NotFound, "no NARInfo found")
	}

	if niResp.StatusCode < 200 || niResp.StatusCode >= 300 {
		log.WithField("status_code", niResp.StatusCode).Warn("Got non-success when trying to request NARInfo")
		return nil, status.Errorf(codes.Internal, "got status code %v trying to request NARInfo", niResp.StatusCode)
	}

	// parse the NARInfo file.
	narInfo, err := narinfo.Parse(niResp.Body)
	if err != nil {
		log.WithError(err).Warn("Unable to parse NARInfo")
		return nil, status.Errorf(codes.Internal, "unable to parse NARInfo")
	}

	// close niResp.Body, we're not gonna read from there anymore.
	_ = niResp.Body.Close()

	// validate the NARInfo file. This ensures strings we need to parse actually parse,
	// so we can just plain panic further down.
	if err := narInfo.Check(); err != nil {
		log.WithError(err).Warn("unable to validate NARInfo")
		return nil, status.Errorf(codes.Internal, "unable to validate NARInfo: %s", err)
	}

	// only allow sha256 here. Is anything else even supported by Nix?
	if narInfo.NarHash.HashType != mh.SHA2_256 {
		log.Error("unsupported hash type")
		return nil, status.Errorf(codes.Internal, "unsuported hash type in NarHash: %s", narInfo.NarHash.SRIString())
	}

	// TODO: calculate fingerprint, check with trusted pubkeys, decide what to do on mismatch

	log = log.WithField("narinfo_narhash", narInfo.NarHash.SRIString())
	log = log.WithField("nar_url", narInfo.URL)

	// prepare the GET request for the NAR file.
	narRq, err := http.NewRequestWithContext(ctx, "GET", p.httpEndpoint.JoinPath(narInfo.URL).String(), nil)
	if err != nil {
		log.WithError(err).Error("unable to construct NAR request")
		return nil, status.Errorf(codes.Internal, "unable to construct NAR request")
	}

	log.Info("requesting NAR")
	narResp, err := p.httpClient.Do(narRq)
	if err != nil {
		log.WithError(err).Error("error during NAR request")
		return nil, status.Errorf(codes.Internal, "error during NAR request")
	}
	defer narResp.Body.Close()

	// If we can't access the NAR file that the NARInfo is referring to, this is a store inconsistency.
	// Propagate a more serious Internal error, rather than just a NotFound.
	if narResp.StatusCode == http.StatusNotFound || narResp.StatusCode == http.StatusForbidden {
		log.Error("Unable to find NAR")
		return nil, status.Errorf(codes.Internal, "NAR at URL %s does not exist", narInfo.URL)
	}

	// wrap narResp.Body with some buffer.
	// We already defer closing the http body, so it's ok to loose io.Close here.
	var narBody io.Reader
	narBody = bufio.NewReaderSize(narResp.Body, 10*1024*1024)

	if narInfo.Compression == "none" {
		// Nothing to do
	} else if narInfo.Compression == "xz" {
		narBody, err = xz.NewReader(narBody)
		if err != nil {
			log.WithError(err).Error("failed to open xz")
			return nil, status.Errorf(codes.Internal, "failed to open xz")
		}
	} else {
		log.WithField("nar_compression", narInfo.Compression).Error("unsupported compression")
		return nil, fmt.Errorf("unsupported NAR compression: %s", narInfo.Compression)
	}

	directoriesUploader := importer.NewDirectoriesUploader(ctx, p.directoryServiceClient)
	defer directoriesUploader.Done() //nolint:errcheck

	blobUploaderCb := importer.GenBlobUploaderCb(ctx, p.blobServiceClient)

	rootNode, _, importedNarSha256, err := importer.Import(
		ctx,
		narBody,
		func(blobReader io.Reader) ([]byte, error) {
			blobDigest, err := blobUploaderCb(blobReader)
			if err != nil {
				return nil, err
			}
			log.WithField("blob_digest", base64.StdEncoding.EncodeToString(blobDigest)).Debug("upload blob")
			return blobDigest, nil
		},
		func(directory *castorev1pb.Directory) ([]byte, error) {
			directoryDigest, err := directoriesUploader.Put(directory)
			if err != nil {
				return nil, err
			}
			log.WithField("directory_digest", base64.StdEncoding.EncodeToString(directoryDigest)).Debug("upload directory")
			return directoryDigest, nil
		},
	)

	if err != nil {
		log.WithError(err).Error("error during NAR import")
		return nil, status.Error(codes.Internal, "error during NAR import")
	}

	// Close the directories uploader. This ensures the DirectoryService has
	// properly persisted all Directory messages sent.
	if _, err := directoriesUploader.Done(); err != nil {
		log.WithError(err).Error("error during directory upload")

		return nil, status.Error(codes.Internal, "error during directory upload")
	}

	// Compare NAR hash in the NARInfo with the one we calculated while reading the NAR
	// We don't need to additionally compare the narSize.
	if !bytes.Equal(narInfo.NarHash.Digest(), importedNarSha256) {
		log := log.WithField("imported_nar_sha256", base64.StdEncoding.EncodeToString(importedNarSha256))
		log.Error("imported digest doesn't match NARInfo digest")

		return nil, fmt.Errorf("imported digest doesn't match NARInfo digest")
	}

	// generate PathInfo
	pathInfo, err := importer.GenPathInfo(rootNode, narInfo)
	if err != nil {
		log.WithError(err).Error("uable to generate PathInfo")
		return nil, status.Errorf(codes.Internal, "unable to generate PathInfo")
	}

	return pathInfo, nil

	// TODO: Deriver, System, CA
}

// List implements storev1.PathInfoServiceServer.
// It returns a permission denied, because normally you can't get a listing
func (*PathInfoServiceServer) List(*storev1pb.ListPathInfoRequest, storev1pb.PathInfoService_ListServer) error {
	return status.Error(codes.Unimplemented, "unimplemented")
}

// Put implements storev1.PathInfoServiceServer.
func (*PathInfoServiceServer) Put(context.Context, *storev1pb.PathInfo) (*storev1pb.PathInfo, error) {
	return nil, status.Error(codes.Unimplemented, "unimplemented")
}
