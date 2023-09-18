package main

import (
	"os"
	"os/signal"

	"github.com/alecthomas/kong"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"code.tvl.fyi/tvix/nar-bridge/pkg/server"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/sirupsen/logrus"
	log "github.com/sirupsen/logrus"
)

// `help:"Expose a tvix-store gRPC Interface as HTTP NAR/NARinfo"`
var cli struct {
	LogLevel        string `enum:"trace,debug,info,warn,error,fatal,panic" help:"The log level to log with" default:"info"`
	ListenAddr      string `name:"listen-addr" help:"The address this service listens on" type:"string" default:"[::]:9000"` //nolint:lll
	EnableAccessLog bool   `name:"access-log" help:"Enable access logging" type:"bool" default:"true" negatable:""`          //nolint:lll
	StoreAddr       string `name:"store-addr" help:"The address to the tvix-store RPC interface this will connect to"`
}

func main() {
	_ = kong.Parse(&cli)

	logLevel, err := logrus.ParseLevel(cli.LogLevel)
	if err != nil {
		log.Panic("invalid log level")
		return
	}
	logrus.SetLevel(logLevel)

	c := make(chan os.Signal, 1)
	signal.Notify(c, os.Interrupt)

	go func() {
		for range c {
			log.Info("Received Signal, shutting down…")
			os.Exit(1)
		}
	}()

	// connect to tvix-store
	log.Debugf("Dialing to %v", cli.StoreAddr)
	conn, err := grpc.Dial(cli.StoreAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("did not connect: %v", err)
	}
	defer conn.Close()

	log.Printf("Starting nar-bridge at %v", cli.ListenAddr)
	s := server.New(
		storev1pb.NewDirectoryServiceClient(conn),
		storev1pb.NewBlobServiceClient(conn),
		storev1pb.NewPathInfoServiceClient(conn),
		cli.EnableAccessLog,
		30,
	)

	err = s.ListenAndServe(cli.ListenAddr)
	if err != nil {
		log.Error("Server failed: %w", err)
		os.Exit(1)
	}
}
