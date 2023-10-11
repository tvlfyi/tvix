package http

import (
	"bufio"
	"bytes"
	"fmt"
	"net/http"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"code.tvl.fyi/tvix/nar-bridge/pkg/importer"
	"github.com/go-chi/chi/v5"
	nixhash "github.com/nix-community/go-nix/pkg/hash"
	"github.com/sirupsen/logrus"
	log "github.com/sirupsen/logrus"
)

func registerNarPut(s *Server) {
	s.handler.Put(narUrl, func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()

		ctx := r.Context()

		// parse the narhash sent in the request URL
		narHashFromUrl, err := parseNarHashFromUrl(chi.URLParamFromCtx(ctx, "narhash"))
		if err != nil {
			log.WithError(err).WithField("url", r.URL).Error("unable to decode nar hash from url")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("unable to decode nar hash from url"))
			if err != nil {
				log.WithError(err).Error("unable to write error message to client")
			}

			return
		}

		log := log.WithField("narhash_url", narHashFromUrl.SRIString())

		directoriesUploader := importer.NewDirectoriesUploader(ctx, s.directoryServiceClient)
		defer directoriesUploader.Done() //nolint:errcheck

		rootNode, narSize, narSha256, err := importer.Import(
			ctx,
			// buffer the body by 10MiB
			bufio.NewReaderSize(r.Body, 10*1024*1024),
			importer.GenBlobUploaderCb(ctx, s.blobServiceClient),
			func(directory *castorev1pb.Directory) ([]byte, error) {
				return directoriesUploader.Put(directory)
			},
		)

		if err != nil {
			log.Errorf("error during NAR import: %v", err)
			w.WriteHeader(http.StatusInternalServerError)
			_, err := w.Write([]byte(fmt.Sprintf("error during NAR import: %v", err)))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		log.Debug("closing the stream")

		// Close the directories uploader
		directoriesPutResponse, err := directoriesUploader.Done()
		if err != nil {
			log.WithError(err).Error("error during directory upload")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("error during directory upload"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}
		// If we uploaded directories (so directoriesPutResponse doesn't return null),
		// the RootDigest field in directoriesPutResponse should match the digest
		// returned in the PathInfo struct returned by the `Import` call.
		// This check ensures the server-side came up with the same root hash.

		if directoriesPutResponse != nil {
			rootDigestPathInfo := rootNode.GetDirectory().GetDigest()
			rootDigestDirectoriesPutResponse := directoriesPutResponse.GetRootDigest()

			log := log.WithFields(logrus.Fields{
				"root_digest_pathinfo":             rootDigestPathInfo,
				"root_digest_directories_put_resp": rootDigestDirectoriesPutResponse,
			})
			if !bytes.Equal(rootDigestPathInfo, rootDigestDirectoriesPutResponse) {
				log.Errorf("returned root digest doesn't match what's calculated")

				w.WriteHeader(http.StatusBadRequest)
				_, err := w.Write([]byte("error in root digest calculation"))
				if err != nil {
					log.WithError(err).Error("unable to write error message to client")
				}

				return
			}
		}

		// Compare the nar hash specified in the URL with the one that has been
		// calculated while processing the NAR file.
		narHash, err := nixhash.FromHashTypeAndDigest(0x12, narSha256)
		if err != nil {
			panic("must parse nixbase32")
		}

		if !bytes.Equal(narHashFromUrl.Digest(), narHash.Digest()) {
			log := log.WithFields(logrus.Fields{
				"narhash_received_sha256": narHash.SRIString(),
				"narsize":                 narSize,
			})
			log.Error("received bytes don't match narhash from URL")

			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("received bytes don't match narHash specified in URL"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		// Insert the partial pathinfo structs into our lookup map,
		// so requesting the NAR file will be possible.
		// The same  might exist already, but it'll have the same contents (so
		// replacing will be a no-op), except maybe the root node Name field value, which
		// is safe to ignore (as not part of the NAR).
		s.narDbMu.Lock()
		s.narDb[narHash.SRIString()] = &narData{
			rootNode: rootNode,
			narSize:  narSize,
		}
		s.narDbMu.Unlock()

		// Done!
	})

}
