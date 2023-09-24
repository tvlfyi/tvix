package main

import (
	"context"
	"os"
	"os/signal"
	"time"

	"github.com/alecthomas/kong"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"code.tvl.fyi/tvix/nar-bridge/pkg/server"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/sirupsen/logrus"
	log "github.com/sirupsen/logrus"
)

// `help:"Expose a tvix-store gRPC Interface as HTTP NAR/NARinfo"`
var cli struct {
	LogLevel        string `enum:"trace,debug,info,warn,error,fatal,panic" help:"The log level to log with" default:"info"`
	ListenAddr      string `name:"listen-addr" help:"The address this service listens on" type:"string" default:"[::]:9000"`                    //nolint:lll
	EnableAccessLog bool   `name:"access-log" help:"Enable access logging" type:"bool" default:"true" negatable:""`                             //nolint:lll
	StoreAddr       string `name:"store-addr" help:"The address to the tvix-store RPC interface this will connect to" default:"localhost:8000"` //nolint:lll
}

func main() {
	_ = kong.Parse(&cli)

	logLevel, err := logrus.ParseLevel(cli.LogLevel)
	if err != nil {
		log.Panic("invalid log level")
		return
	}
	logrus.SetLevel(logLevel)

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	// connect to tvix-store
	log.Debugf("Dialing to %v", cli.StoreAddr)
	conn, err := grpc.DialContext(ctx, cli.StoreAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("did not connect: %v", err)
	}
	defer conn.Close()

	s := server.New(
		castorev1pb.NewDirectoryServiceClient(conn),
		castorev1pb.NewBlobServiceClient(conn),
		storev1pb.NewPathInfoServiceClient(conn),
		cli.EnableAccessLog,
		30,
	)

	log.Printf("Starting nar-bridge at %v", cli.ListenAddr)
	go s.ListenAndServe(cli.ListenAddr)

	// listen for the interrupt signal.
	<-ctx.Done()

	// Restore default behaviour on the interrupt signal
	stop()
	log.Info("Received Signal, shutting down, press Ctl+C again to force.")

	timeoutCtx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	if s.Shutdown(timeoutCtx); err != nil {
		log.WithError(err).Warn("failed to shutdown")
		os.Exit(1)
	}
}
