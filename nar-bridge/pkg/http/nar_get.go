package http

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"sync"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/go-chi/chi/v5"
	nixhash "github.com/nix-community/go-nix/pkg/hash"
	"github.com/nix-community/go-nix/pkg/nixbase32"
	log "github.com/sirupsen/logrus"
)

const (
	narUrl = "/nar/{narhash:^([" + nixbase32.Alphabet + "]{52})$}.nar"
)

func renderNar(
	ctx context.Context,
	log *log.Entry,
	directoryServiceClient castorev1pb.DirectoryServiceClient,
	blobServiceClient castorev1pb.BlobServiceClient,
	narHashToPathInfoMu *sync.Mutex,
	narHashToPathInfo map[string]*storev1pb.PathInfo,
	w io.Writer,
	narHash *nixhash.Hash,
	headOnly bool,
) error {
	// look in the lookup table
	narHashToPathInfoMu.Lock()
	pathInfo, found := narHashToPathInfo[narHash.SRIString()]
	narHashToPathInfoMu.Unlock()

	// if we didn't find anything, return 404.
	if !found {
		return fmt.Errorf("narHash not found: %w", fs.ErrNotExist)
	}

	// if this was only a head request, we're done.
	if headOnly {
		return nil
	}

	directories := make(map[string]*castorev1pb.Directory)

	// If the root node is a directory, ask the directory service for all directories
	if pathInfoDirectory := pathInfo.GetNode().GetDirectory(); pathInfoDirectory != nil {
		rootDirectoryDigest := pathInfoDirectory.GetDigest()
		log = log.WithField("root_directory", base64.StdEncoding.EncodeToString(rootDirectoryDigest))

		directoryStream, err := directoryServiceClient.Get(ctx, &castorev1pb.GetDirectoryRequest{
			ByWhat: &castorev1pb.GetDirectoryRequest_Digest{
				Digest: rootDirectoryDigest,
			},
			Recursive: true,
		})
		if err != nil {
			return fmt.Errorf("unable to query directory stream: %w", err)
		}

		// For now, we just stream all of these locally and put them into a hashmap,
		// which is used in the lookup function below.
		for {
			directory, err := directoryStream.Recv()
			if err != nil {
				if err == io.EOF {
					break
				}
				return fmt.Errorf("unable to receive from directory stream: %w", err)
			}

			// calculate directory digest
			// TODO: do we need to do any more validation?
			directoryDgst, err := directory.Digest()
			if err != nil {
				return fmt.Errorf("unable to calculate directory digest: %w", err)
			}

			log.WithField("directory", base64.StdEncoding.EncodeToString(directoryDgst)).Debug("received directory node")

			directories[hex.EncodeToString(directoryDgst)] = directory
		}

	}

	// render the NAR file
	err := storev1pb.Export(
		w,
		pathInfo.Node,
		func(directoryDigest []byte) (*castorev1pb.Directory, error) {
			log.WithField("directory", base64.StdEncoding.EncodeToString(directoryDigest)).Debug("Get directory")
			directoryRefStr := hex.EncodeToString(directoryDigest)
			directory, found := directories[directoryRefStr]
			if !found {
				return nil, fmt.Errorf(
					"directory with hash %v does not exist: %w",
					directoryDigest,
					fs.ErrNotExist,
				)
			}

			return directory, nil
		},
		func(blobDigest []byte) (io.ReadCloser, error) {
			log.WithField("blob", base64.StdEncoding.EncodeToString(blobDigest)).Debug("Get blob")
			resp, err := blobServiceClient.Read(ctx, &castorev1pb.ReadBlobRequest{
				Digest: blobDigest,
			})
			if err != nil {
				return nil, fmt.Errorf("unable to get blob: %w", err)
			}

			// set up a pipe, let a goroutine write, return the reader.
			pR, pW := io.Pipe()

			go func() {
				for {
					chunk, err := resp.Recv()
					if errors.Is(err, io.EOF) {
						break
					}
					if err != nil {
						pW.CloseWithError(fmt.Errorf("receiving chunk: %w", err))
						return
					}

					// write the received chunk to the writer part of the pipe
					if _, err := io.Copy(pW, bytes.NewReader(chunk.GetData())); err != nil {
						log.WithError(err).Error("writing chunk to pipe")
						pW.CloseWithError(fmt.Errorf("writing chunk to pipe: %w", err))
						return
					}
				}
				pW.Close()

			}()

			return io.NopCloser(pR), nil
		},
	)
	if err != nil {
		return fmt.Errorf("unable to export nar: %w", err)
	}
	return nil
}

func registerNarGet(s *Server) {
	// produce a handler for rendering NAR files.
	genNarHandler := func(isHead bool) func(w http.ResponseWriter, r *http.Request) {
		return func(w http.ResponseWriter, r *http.Request) {
			defer r.Body.Close()

			ctx := r.Context()

			// parse the narhash sent in the request URL
			narHash, err := parseNarHashFromUrl(chi.URLParamFromCtx(ctx, "narhash"))
			if err != nil {
				log.WithError(err).WithField("url", r.URL).Error("unable to decode nar hash from url")
				w.WriteHeader(http.StatusBadRequest)
				_, err := w.Write([]byte("unable to decode nar hash from url"))
				if err != nil {
					log.WithError(err).Errorf("unable to write error message to client")
				}

				return
			}

			log := log.WithField("narhash_url", narHash.SRIString())

			// TODO: inline more of that function here?
			err = renderNar(ctx, log, s.directoryServiceClient, s.blobServiceClient, &s.narHashToPathInfoMu, s.narHashToPathInfo, w, narHash, isHead)
			if err != nil {
				if errors.Is(err, fs.ErrNotExist) {
					w.WriteHeader(http.StatusNotFound)
				} else {
					log.WithError(err).Warn("unable to render nar")
					w.WriteHeader(http.StatusInternalServerError)
				}
			}

		}
	}

	s.handler.Head(narUrl, genNarHandler(true))
	s.handler.Get(narUrl, genNarHandler(false))
}
