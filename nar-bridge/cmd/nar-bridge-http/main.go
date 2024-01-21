package main

import (
	"context"
	"os"
	"os/signal"
	"runtime/debug"
	"time"

	"github.com/alecthomas/kong"

	"go.opentelemetry.io/contrib/instrumentation/google.golang.org/grpc/otelgrpc"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	castorev1pb "code.tvl.fyi/tvix/castore-go"
	narBridgeHttp "code.tvl.fyi/tvix/nar-bridge/pkg/http"
	storev1pb "code.tvl.fyi/tvix/store-go"
	log "github.com/sirupsen/logrus"
)

// `help:"Expose a tvix-store gRPC Interface as HTTP NAR/NARinfo"`
var cli struct {
	LogLevel        string `enum:"trace,debug,info,warn,error,fatal,panic" help:"The log level to log with" default:"info"`
	ListenAddr      string `name:"listen-addr" help:"The address this service listens on" type:"string" default:"[::]:9000"`                    //nolint:lll
	EnableAccessLog bool   `name:"access-log" help:"Enable access logging" type:"bool" default:"true" negatable:""`                             //nolint:lll
	StoreAddr       string `name:"store-addr" help:"The address to the tvix-store RPC interface this will connect to" default:"localhost:8000"` //nolint:lll
	EnableOtlp      bool   `name:"otlp" help:"Enable OpenTelemetry for logs, spans, and metrics" default:"true"`                                //nolint:lll
}

func main() {
	_ = kong.Parse(&cli)

	logLevel, err := log.ParseLevel(cli.LogLevel)
	if err != nil {
		log.Panic("invalid log level")
		return
	}
	log.SetLevel(logLevel)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	if cli.EnableOtlp {
		buildInfo, ok := debug.ReadBuildInfo()
		if !ok {
			log.Fatal("failed to read build info")
		}

		shutdown, err := setupOpenTelemetry(ctx, "nar-bridge", buildInfo.Main.Version)
		if err != nil {
			log.WithError(err).Fatal("failed to setup OpenTelemetry")
		}
		defer shutdown(context.Background())
	}

	// connect to tvix-store
	log.Debugf("Dialing to %v", cli.StoreAddr)
	conn, err := grpc.DialContext(ctx, cli.StoreAddr,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithStatsHandler(otelgrpc.NewClientHandler()),
	)
	if err != nil {
		log.Fatalf("did not connect: %v", err)
	}
	defer conn.Close()

	s := narBridgeHttp.New(
		castorev1pb.NewDirectoryServiceClient(conn),
		castorev1pb.NewBlobServiceClient(conn),
		storev1pb.NewPathInfoServiceClient(conn),
		cli.EnableAccessLog,
		30,
	)

	log.Printf("Starting nar-bridge-http at %v", cli.ListenAddr)
	go s.ListenAndServe(cli.ListenAddr)

	// listen for the interrupt signal.
	<-ctx.Done()

	// Restore default behaviour on the interrupt signal
	stop()
	log.Info("Received Signal, shutting down, press Ctl+C again to force.")

	timeoutCtx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	if err := s.Shutdown(timeoutCtx); err != nil {
		log.WithError(err).Warn("failed to shutdown")
		os.Exit(1)
	}
}
