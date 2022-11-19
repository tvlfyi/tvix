package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"os/signal"

	storev1pb "code.tvl.fyi/tvix/store/protos"

	"code.tvl.fyi/tvix/nar-bridge/pkg/reader"
	log "github.com/sirupsen/logrus"
)

type ImportCmd struct {
	NarPath string `name:"nar-path" help:"A path to a NAR file"`
}

// `help:"Read a NAR file and display some information"`

func (cmd *ImportCmd) Run() error {
	retcode := 0

	defer func() { os.Exit(retcode) }()

	c := make(chan os.Signal, 1)
	signal.Notify(c, os.Interrupt)

	go func() {
		for range c {
			log.Info("Received Signal, shutting down…")
			os.Exit(1)
		}
	}()

	log.Infof("Reading %v...", cmd.NarPath)

	f, _ := os.Open(cmd.NarPath)

	r := reader.New(f)

	actualPathInfo, _ := r.Import(
		context.Background(),
		func(fileReader io.Reader) error {
			return nil
		},
		func(directory *storev1pb.Directory) error {
			return nil
		},
	)

	fmt.Printf("Node: %+v\n", actualPathInfo.Node)
	fmt.Printf("References: %+v\n", actualPathInfo.References)
	fmt.Printf("Narinfo: %+v\n", actualPathInfo.Narinfo)
	return nil
}
