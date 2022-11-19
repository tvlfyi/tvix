package main

import (
	"os"

	"github.com/alecthomas/kong"
)

//nolint:gochecknoglobals
var cli struct {
	// TODO: make log level configurable
	Import ImportCmd `kong:"cmd,name='import',help='Import a local NAR file into a tvix-store'"`
	Serve  ServeCmd  `kong:"cmd,name='serve',help='Expose a tvix-store RPC interface as NAR/NARInfo'"`
}

func main() {
	parser, err := kong.New(&cli)
	if err != nil {
		panic(err)
	}

	ctx, err := parser.Parse(os.Args[1:])
	if err != nil {
		panic(err)
	}
	// Call the Run() method of the selected parsed command.
	err = ctx.Run()

	ctx.FatalIfErrorf(err)
}
