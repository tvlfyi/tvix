package http

import (
	"fmt"
	nixhash "github.com/nix-community/go-nix/pkg/hash"
)

// parseNarHashFromUrl parses a nixbase32 string representing a sha256 NarHash
// and returns a nixhash.Hash when it was able to parse, or an error.
func parseNarHashFromUrl(narHashFromUrl string) (*nixhash.Hash, error) {
	// peek at the length. If it's 52 characters, assume sha256,
	// if it's something else, this is an error.
	l := len(narHashFromUrl)
	if l != 52 {
		return nil, fmt.Errorf("invalid length of narHash: %v", l)
	}

	nixHash, err := nixhash.ParseNixBase32("sha256:" + narHashFromUrl)
	if err != nil {
		return nil, fmt.Errorf("unable to parse nixbase32 hash: %w", err)
	}

	return nixHash, nil
}
