package http

import (
	"fmt"

	storev1pb "code.tvl.fyi/tvix/store/protos"
	mh "github.com/multiformats/go-multihash/core"
	nixhash "github.com/nix-community/go-nix/pkg/hash"

	"github.com/nix-community/go-nix/pkg/narinfo"
	"github.com/nix-community/go-nix/pkg/narinfo/signature"
	"github.com/nix-community/go-nix/pkg/nixbase32"
)

// ToNixNarInfo converts the PathInfo to a narinfo.NarInfo.
func ToNixNarInfo(p *storev1pb.PathInfo) (*narinfo.NarInfo, error) {
	// ensure the PathInfo is valid, and extract the StorePath from the node in
	// there.
	storePath, err := p.Validate()
	if err != nil {
		return nil, fmt.Errorf("failed to validate PathInfo: %w", err)
	}

	// convert the signatures from storev1pb signatures to narinfo signatures
	narinfoSignatures := make([]signature.Signature, len(p.GetNarinfo().GetSignatures()))
	for i, pathInfoSignature := range p.GetNarinfo().GetSignatures() {
		narinfoSignatures[i] = signature.Signature{
			Name: pathInfoSignature.GetName(),
			Data: pathInfoSignature.GetData(),
		}
	}

	// produce nixhash for the narsha256.
	narHash, err := nixhash.FromHashTypeAndDigest(
		mh.SHA2_256,
		p.GetNarinfo().GetNarSha256(),
	)
	if err != nil {
		return nil, fmt.Errorf("invalid narsha256: %w", err)
	}

	return &narinfo.NarInfo{
		StorePath:   storePath.Absolute(),
		URL:         "nar/" + nixbase32.EncodeToString(narHash.Digest()) + ".nar",
		Compression: "none",
		NarHash:     narHash,
		NarSize:     uint64(p.GetNarinfo().GetNarSize()),
		References:  p.GetNarinfo().GetReferenceNames(),
		Signatures:  narinfoSignatures,
	}, nil
}
