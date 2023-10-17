package main

import (
	"context"
	"fmt"
	"net"
	"net/http"
	"net/url"
	"os"
	"os/signal"
	"strings"
	"time"

	"github.com/alecthomas/kong"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/reflection"

	castorev1pb "code.tvl.fyi/tvix/castore-go"
	"code.tvl.fyi/tvix/nar-bridge/pkg/pathinfosvc"
	storev1pb "code.tvl.fyi/tvix/store-go"
	"github.com/sirupsen/logrus"
	log "github.com/sirupsen/logrus"
)

// `help:"Provide a tvix-store gRPC PathInfoService for a HTTP Nix Binary Cache"`
var cli struct {
	LogLevel             string   `enum:"trace,debug,info,warn,error,fatal,panic" help:"The log level to log with" default:"info"`
	ListenAddr           string   `name:"listen-addr" help:"The address this service listens on" type:"string" default:"[::]:8001"` //nolint:lll
	BlobServiceAddr      string   `name:"blob-service-addr" env:"BLOB_SERVICE_ADDR" default:"grpc+http://[::1]:8000"`
	DirectoryServiceAddr string   `name:"directory-service-addr" env:"DIRECTORY_SERVICE_ADDR" default:"grpc+http://[::1]:8000"`
	HTTPBinaryCacheURL   *url.URL `name:"http-binary-cache-url" env:"HTTP_BINARY_CACHE_URL" help:"The URL containing the Nix HTTP Binary cache" default:"https://cache.nixos.org"`
}

func connectService(ctx context.Context, serviceAddr string) (*grpc.ClientConn, error) {
	if !strings.HasPrefix(serviceAddr, "grpc+http://") {
		return nil, fmt.Errorf("invalid serviceAddr: %s", serviceAddr)
	}
	addr := strings.TrimPrefix(serviceAddr, "grpc+http://")

	conn, err := grpc.DialContext(ctx, addr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("did not connect: %v", err)
	}
	return conn, nil
}

func main() {
	_ = kong.Parse(&cli)

	logLevel, err := logrus.ParseLevel(cli.LogLevel)
	if err != nil {
		log.Fatal("invalid log level")
	}
	logrus.SetLevel(logLevel)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	// connect to the two stores
	connBlobSvc, err := connectService(ctx, cli.BlobServiceAddr)
	if err != nil {
		log.Fatalf("unable to connect to blob service: %v", err)
	}
	defer connBlobSvc.Close()

	connDirectorySvc, err := connectService(ctx, cli.DirectoryServiceAddr)
	if err != nil {
		log.Fatalf("unable to connect to directory service: %v", err)
	}
	defer connDirectorySvc.Close()

	// set up pathinfoservice
	var opts []grpc.ServerOption
	s := grpc.NewServer(opts...)
	reflection.Register(s)

	storev1pb.RegisterPathInfoServiceServer(s,
		pathinfosvc.New(
			cli.HTTPBinaryCacheURL,
			&http.Client{},
			castorev1pb.NewDirectoryServiceClient(connDirectorySvc),
			castorev1pb.NewBlobServiceClient(connBlobSvc),
		),
	)

	log.Printf("Starting nar-bridge-pathinfosvc at %v", cli.ListenAddr)
	lis, err := net.Listen("tcp", cli.ListenAddr)
	if err != nil {
		log.Fatalf("failed to listen: %v", err)
	}
	go s.Serve(lis)

	// listen for the interrupt signal.
	<-ctx.Done()

	// Restore default behaviour on the interrupt signal
	stop()
	log.Info("Received Signal, shutting down, press Ctl+C again to force.")

	stopped := make(chan interface{})
	go func() {
		s.GracefulStop()
		close(stopped)
	}()

	t := time.NewTimer(30 * time.Second)
	select {
	case <-t.C:
		log.Info("timeout, kicking remaining clients")
		s.Stop()
	case <-stopped:
		log.Info("all clients left during grace period")
		t.Stop()
	}
}
