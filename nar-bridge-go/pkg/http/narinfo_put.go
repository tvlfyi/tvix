package http

import (
	"net/http"

	"code.tvl.fyi/tvix/nar-bridge-go/pkg/importer"
	"github.com/go-chi/chi/v5"
	"github.com/nix-community/go-nix/pkg/narinfo"
	"github.com/nix-community/go-nix/pkg/nixbase32"
	"github.com/sirupsen/logrus"
	log "github.com/sirupsen/logrus"
)

func registerNarinfoPut(s *Server) {
	s.handler.Put("/{outputhash:^["+nixbase32.Alphabet+"]{32}}.narinfo", func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()

		ctx := r.Context()
		log := log.WithField("outputhash", chi.URLParamFromCtx(ctx, "outputhash"))

		// TODO: decide on merging behaviour.
		// Maybe it's fine to add if contents are the same, but more sigs can be added?
		// Right now, just replace a .narinfo for a path that already exists.

		// read and parse the .narinfo file
		narInfo, err := narinfo.Parse(r.Body)
		if err != nil {
			log.WithError(err).Error("unable to parse narinfo")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("unable to parse narinfo"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		log = log.WithFields(logrus.Fields{
			"narhash":     narInfo.NarHash.SRIString(),
			"output_path": narInfo.StorePath,
		})

		// look up the narHash in our temporary map
		s.narDbMu.Lock()
		narData, found := s.narDb[narInfo.NarHash.SRIString()]
		s.narDbMu.Unlock()
		if !found {
			log.Error("unable to find referred NAR")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("unable to find referred NAR"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		rootNode := narData.rootNode

		// compare fields with what we computed while receiving the NAR file

		// NarSize needs to match
		if narData.narSize != narInfo.NarSize {
			log.Error("narsize mismatch")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("unable to parse narinfo"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		pathInfo, err := importer.GenPathInfo(rootNode, narInfo)
		if err != nil {
			log.WithError(err).Error("unable to generate PathInfo")

			w.WriteHeader(http.StatusInternalServerError)
			_, err := w.Write([]byte("unable to generate PathInfo"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		log.WithField("pathInfo", pathInfo).Debug("inserted new pathInfo")

		receivedPathInfo, err := s.pathInfoServiceClient.Put(ctx, pathInfo)
		if err != nil {
			log.WithError(err).Error("unable to upload pathinfo to service")
			w.WriteHeader(http.StatusInternalServerError)
			_, err := w.Write([]byte("unable to upload pathinfo to server"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		log.WithField("pathInfo", receivedPathInfo).Debug("got back PathInfo")
	})
}
