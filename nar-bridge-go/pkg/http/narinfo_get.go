package http

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"strings"
	"sync"

	storev1pb "code.tvl.fyi/tvix/store-go"
	"github.com/go-chi/chi/v5"
	nixhash "github.com/nix-community/go-nix/pkg/hash"
	"github.com/nix-community/go-nix/pkg/nixbase32"
	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// renderNarinfo writes narinfo contents to a passed io.Writer, or a returns a
// (wrapped) io.ErrNoExist error if something doesn't exist.
// if headOnly is set to true, only the existence is checked, but no content is
// actually written.
func renderNarinfo(
	ctx context.Context,
	log *log.Entry,
	pathInfoServiceClient storev1pb.PathInfoServiceClient,
	narHashToPathInfoMu *sync.Mutex,
	narHashToPathInfo map[string]*narData,
	outputHash []byte,
	w io.Writer,
	headOnly bool,
) error {
	pathInfo, err := pathInfoServiceClient.Get(ctx, &storev1pb.GetPathInfoRequest{
		ByWhat: &storev1pb.GetPathInfoRequest_ByOutputHash{
			ByOutputHash: outputHash,
		},
	})
	if err != nil {
		st, ok := status.FromError(err)
		if ok {
			if st.Code() == codes.NotFound {
				return fmt.Errorf("output hash %v not found: %w", base64.StdEncoding.EncodeToString(outputHash), fs.ErrNotExist)
			}
			return fmt.Errorf("unable to get pathinfo, code %v: %w", st.Code(), err)
		}

		return fmt.Errorf("unable to get pathinfo: %w", err)
	}

	log = log.WithField("pathInfo", pathInfo)

	if _, err := pathInfo.Validate(); err != nil {
		log.WithError(err).Error("unable to validate PathInfo")

		return fmt.Errorf("unable to validate PathInfo: %w", err)
	}

	if pathInfo.GetNarinfo() == nil {
		log.Error("PathInfo doesn't contain Narinfo field")

		return fmt.Errorf("PathInfo doesn't contain Narinfo field")
	}

	// extract the NARHash. This must succeed, as Validate() did succeed.
	narHash, err := nixhash.FromHashTypeAndDigest(0x12, pathInfo.GetNarinfo().GetNarSha256())
	if err != nil {
		panic("must parse NarHash")
	}

	// add things to the lookup table, in case the same process didn't handle the NAR hash yet.
	narHashToPathInfoMu.Lock()
	narHashToPathInfo[narHash.SRIString()] = &narData{
		rootNode: pathInfo.GetNode(),
		narSize:  pathInfo.GetNarinfo().GetNarSize(),
	}
	narHashToPathInfoMu.Unlock()

	if headOnly {
		return nil
	}

	// convert the PathInfo to NARInfo.
	narInfo, err := ToNixNarInfo(pathInfo)

	// Write it out to the client.
	_, err = io.Copy(w, strings.NewReader(narInfo.String()))
	if err != nil {
		return fmt.Errorf("unable to write narinfo to client: %w", err)
	}

	return nil
}

func registerNarinfoGet(s *Server) {
	// GET/HEAD $outHash.narinfo looks up the PathInfo from the tvix-store,
	// and, if it's a GET request, render a .narinfo file to the client.
	// In both cases it will keep the PathInfo in the lookup map,
	// so a subsequent GET/HEAD /nar/ $narhash.nar request can find it.
	genNarinfoHandler := func(isHead bool) func(w http.ResponseWriter, r *http.Request) {
		return func(w http.ResponseWriter, r *http.Request) {
			defer r.Body.Close()

			ctx := r.Context()
			log := log.WithField("outputhash", chi.URLParamFromCtx(ctx, "outputhash"))

			// parse the output hash sent in the request URL
			outputHash, err := nixbase32.DecodeString(chi.URLParamFromCtx(ctx, "outputhash"))
			if err != nil {
				log.WithError(err).Error("unable to decode output hash from url")
				w.WriteHeader(http.StatusBadRequest)
				_, err := w.Write([]byte("unable to decode output hash from url"))
				if err != nil {
					log.WithError(err).Errorf("unable to write error message to client")
				}

				return
			}

			err = renderNarinfo(ctx, log, s.pathInfoServiceClient, &s.narDbMu, s.narDb, outputHash, w, isHead)
			if err != nil {
				if errors.Is(err, fs.ErrNotExist) {
					w.WriteHeader(http.StatusNotFound)
				} else {
					log.WithError(err).Warn("unable to render narinfo")
					w.WriteHeader(http.StatusInternalServerError)
				}
			}
		}
	}

	s.handler.Get("/{outputhash:^["+nixbase32.Alphabet+"]{32}}.narinfo", genNarinfoHandler(false))
	s.handler.Head("/{outputhash:^["+nixbase32.Alphabet+"]{32}}.narinfo", genNarinfoHandler(true))
}
