package main

import (
	"os"
	"os/signal"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"code.tvl.fyi/tvix/nar-bridge/pkg/server"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	log "github.com/sirupsen/logrus"
)

type ServeCmd struct {
	ListenAddr      string `name:"listen-addr" help:"The address this service listens on" type:"string" default:"[::]:9000"` //nolint:lll
	EnableAccessLog bool   `name:"access-log" help:"Enable access logging" type:"bool" default:"true" negatable:""`          //nolint:lll
	StoreAddr       string `name:"store-addr" help:"The address to the tvix-store RPC interface this will connect to"`
}

// `help:"Expose a tvix-store RPC interface as NAR/NARInfo"`
func (cmd *ServeCmd) Run() error {
	retcode := 0

	defer func() { os.Exit(retcode) }()

	c := make(chan os.Signal, 1)
	signal.Notify(c, os.Interrupt)

	go func() {
		for range c {
			log.Info("Received Signal, shutting down…")
			//s.Close()
			os.Exit(1)
		}
	}()

	// connect to tvix-store
	log.Debugf("Dialing to %v", cmd.StoreAddr)
	conn, err := grpc.Dial(cmd.StoreAddr, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		log.Fatalf("did not connect: %v", err)
	}
	defer conn.Close()

	log.Printf("Starting nar-bridge at %v", cmd.ListenAddr)
	s := server.New(
		storev1pb.NewDirectoryServiceClient(conn),
		storev1pb.NewBlobServiceClient(conn),
		storev1pb.NewPathInfoServiceClient(conn),
		cmd.EnableAccessLog,
		30,
	)

	err = s.ListenAndServe(cmd.ListenAddr)
	if err != nil {
		log.Error("Server failed: %w", err)
	}
	return nil
}
