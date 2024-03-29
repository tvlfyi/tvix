package http

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"strings"
	"sync"
	"time"

	castorev1pb "code.tvl.fyi/tvix/castore-go"
	storev1pb "code.tvl.fyi/tvix/store-go"
	"github.com/go-chi/chi/middleware"
	"github.com/go-chi/chi/v5"
	log "github.com/sirupsen/logrus"
	"go.opentelemetry.io/contrib/instrumentation/net/http/otelhttp"
)

type Server struct {
	srv     *http.Server
	handler chi.Router

	directoryServiceClient castorev1pb.DirectoryServiceClient
	blobServiceClient      castorev1pb.BlobServiceClient
	pathInfoServiceClient  storev1pb.PathInfoServiceClient

	// When uploading NAR files to a HTTP binary cache, the .nar
	// files are uploaded before the .narinfo files.
	// We need *both* to be able to fully construct a PathInfo object.
	// Keep a in-memory map of narhash(es) (in SRI) to (unnamed) root node and nar
	// size.
	// This is necessary until we can ask a PathInfoService for a node with a given
	// narSha256.
	narDbMu sync.Mutex
	narDb   map[string]*narData
}

type narData struct {
	rootNode *castorev1pb.Node
	narSize  uint64
}

func New(
	directoryServiceClient castorev1pb.DirectoryServiceClient,
	blobServiceClient castorev1pb.BlobServiceClient,
	pathInfoServiceClient storev1pb.PathInfoServiceClient,
	enableAccessLog bool,
	priority int,
) *Server {
	r := chi.NewRouter()
	r.Use(func(h http.Handler) http.Handler {
		return otelhttp.NewHandler(h, "http.request")
	})

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
		narDb:                  make(map[string]*narData),
	}

	registerNarPut(s)
	registerNarinfoPut(s)

	registerNarinfoGet(s)
	registerNarGet(s)

	return s
}

func (s *Server) Shutdown(ctx context.Context) error {
	return s.srv.Shutdown(ctx)
}

// ListenAndServer starts the webserver, and waits for it being closed or
// shutdown, after which it'll return ErrServerClosed.
func (s *Server) ListenAndServe(addr string) error {
	s.srv = &http.Server{
		Handler:      s.handler,
		ReadTimeout:  500 * time.Second,
		WriteTimeout: 500 * time.Second,
		IdleTimeout:  500 * time.Second,
	}

	var listener net.Listener
	var err error

	// check addr. If it contains slashes, assume it's a unix domain socket.
	if strings.Contains(addr, "/") {
		listener, err = net.Listen("unix", addr)
	} else {
		listener, err = net.Listen("tcp", addr)
	}
	if err != nil {
		return fmt.Errorf("unable to listen on %v: %w", addr, err)
	}

	return s.srv.Serve(listener)
}
