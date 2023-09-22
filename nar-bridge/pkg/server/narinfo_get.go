package server

import (
	"context"
	"encoding/base64"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/http"
	"path"
	"strings"
	"sync"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/go-chi/chi/v5"
	nixhash "github.com/nix-community/go-nix/pkg/hash"
	"github.com/nix-community/go-nix/pkg/narinfo"
	"github.com/nix-community/go-nix/pkg/narinfo/signature"
	"github.com/nix-community/go-nix/pkg/nixbase32"
	"github.com/nix-community/go-nix/pkg/nixpath"
	log "github.com/sirupsen/logrus"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
)

// renderNarinfo writes narinfo contents to a passes io.Writer, or a returns a
// (wrapped) io.ErrNoExist error if something doesn't exist.
// if headOnly is set to true, only the existence is checked, but no content is
// actually written.
func renderNarinfo(
	ctx context.Context,
	log *log.Entry,
	pathInfoServiceClient storev1pb.PathInfoServiceClient,
	narHashToPathInfoMu *sync.Mutex,
	narHashToPathInfo map[string]*storev1pb.PathInfo,
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

	narHash, err := nixhash.ParseNixBase32("sha256:" + nixbase32.EncodeToString(pathInfo.GetNarinfo().GetNarSha256()))
	if err != nil {
		// TODO: return proper error
		return fmt.Errorf("No usable NarHash found in PathInfo")
	}

	// add things to the lookup table, in case the same process didn't handle the NAR hash yet.
	narHashToPathInfoMu.Lock()
	narHashToPathInfo[narHash.SRIString()] = pathInfo
	narHashToPathInfoMu.Unlock()

	if headOnly {
		return nil
	}

	// convert the signatures from storev1pb signatures to narinfo signatures
	narinfoSignatures := make([]signature.Signature, 0)
	for _, pathInfoSignature := range pathInfo.Narinfo.Signatures {
		narinfoSignatures = append(narinfoSignatures, signature.Signature{
			Name: pathInfoSignature.GetName(),
			Data: pathInfoSignature.GetData(),
		})
	}

	// extract the name of the node in the pathInfo structure, which will become the output path
	var nodeName []byte
	switch v := (pathInfo.GetNode().GetNode()).(type) {
	case *castorev1pb.Node_File:
		nodeName = v.File.GetName()
	case *castorev1pb.Node_Symlink:
		nodeName = v.Symlink.GetName()
	case *castorev1pb.Node_Directory:
		nodeName = v.Directory.GetName()
	}

	narInfo := narinfo.NarInfo{
		StorePath:   path.Join(nixpath.StoreDir, string(nodeName)),
		URL:         "nar/" + nixbase32.EncodeToString(narHash.Digest()) + ".nar",
		Compression: "none", // TODO: implement zstd compression
		NarHash:     narHash,
		NarSize:     uint64(pathInfo.Narinfo.NarSize),
		References:  pathInfo.Narinfo.GetReferenceNames(),
		Signatures:  narinfoSignatures,
	}

	// render .narinfo from pathInfo
	_, err = io.Copy(w, strings.NewReader(narInfo.String()))
	if err != nil {
		return fmt.Errorf("unable to write narinfo to client: %w", err)
	}

	return nil
}

func registerNarinfoGet(s *Server) {
	// GET $outHash.narinfo looks up the PathInfo from the tvix-store,
	// and then render a .narinfo file to the client.
	// It will keep the PathInfo in the lookup map,
	// so a subsequent GET /nar/ $narhash.nar request can find it.
	s.handler.Get("/{outputhash:^["+nixbase32.Alphabet+"]{32}}.narinfo", func(w http.ResponseWriter, r *http.Request) {
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

		err = renderNarinfo(ctx, log, s.pathInfoServiceClient, &s.narHashToPathInfoMu, s.narHashToPathInfo, outputHash, w, false)
		if err != nil {
			if errors.Is(err, fs.ErrNotExist) {
				w.WriteHeader(http.StatusNotFound)
			} else {
				log.WithError(err).Warn("unable to render narinfo")
				w.WriteHeader(http.StatusInternalServerError)
			}
		}
	})
}
