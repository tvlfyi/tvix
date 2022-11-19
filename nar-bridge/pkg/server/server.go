package server

import (
	"fmt"
	"net/http"
	"sync"
	"time"

	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/go-chi/chi/middleware"
	"github.com/go-chi/chi/v5"
	log "github.com/sirupsen/logrus"
)

type Server struct {
	handler chi.Router

	directoryServiceClient storev1pb.DirectoryServiceClient
	blobServiceClient      storev1pb.BlobServiceClient
	pathInfoServiceClient  storev1pb.PathInfoServiceClient

	// When uploading NAR files to a HTTP binary cache, the .nar
	// files are uploaded before the .narinfo files.
	// We need *both* to be able to fully construct a PathInfo object.
	// Keep a in-memory map of narhash(es) (in SRI) to sparse PathInfo.
	// This is necessary until we can ask a PathInfoService for a node with a given
	// narSha256.
	narHashToPathInfoMu sync.Mutex
	narHashToPathInfo   map[string]*storev1pb.PathInfo
}

func New(
	directoryServiceClient storev1pb.DirectoryServiceClient,
	blobServiceClient storev1pb.BlobServiceClient,
	pathInfoServiceClient storev1pb.PathInfoServiceClient,
	enableAccessLog bool,
	priority int,
) *Server {
	r := chi.NewRouter()

	if enableAccessLog {
		r.Use(middleware.Logger)
	}

	r.Get("/", func(w http.ResponseWriter, r *http.Request) {
		_, err := w.Write([]byte("nar-bridge"))
		if err != nil {
			log.Errorf("Unable to write response: %v", err)
		}
	})

	r.Get("/nix-cache-info", func(w http.ResponseWriter, r *http.Request) {
		_, err := w.Write([]byte(fmt.Sprintf("StoreDir: /nix/store\nWantMassQuery: 1\nPriority: %d\n", priority)))
		if err != nil {
			log.Errorf("Unable to write response: %v", err)
		}
	})

	s := &Server{
		handler:                r,
		directoryServiceClient: directoryServiceClient,
		blobServiceClient:      blobServiceClient,
		pathInfoServiceClient:  pathInfoServiceClient,
		narHashToPathInfo:      make(map[string]*storev1pb.PathInfo),
	}

	registerNarPut(s)
	registerNarinfoPut(s)

	registerNarinfoGet(s)
	registerNarGet(s)

	return s
}

func (s *Server) ListenAndServe(addr string) error {
	srv := &http.Server{
		Addr:         addr,
		Handler:      s.handler,
		ReadTimeout:  50 * time.Second,
		WriteTimeout: 100 * time.Second,
		IdleTimeout:  150 * time.Second,
	}

	return srv.ListenAndServe()
}
