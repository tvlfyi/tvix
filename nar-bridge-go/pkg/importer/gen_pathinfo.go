package importer

import (
	castorev1pb "code.tvl.fyi/tvix/castore-go"
	storev1pb "code.tvl.fyi/tvix/store-go"
	"fmt"
	"github.com/nix-community/go-nix/pkg/narinfo"
	"github.com/nix-community/go-nix/pkg/storepath"
)

// GenPathInfo takes a rootNode and narInfo and assembles a PathInfo.
// The rootNode is renamed to match the StorePath in the narInfo.
func GenPathInfo(rootNode *castorev1pb.Node, narInfo *narinfo.NarInfo) (*storev1pb.PathInfo, error) {
	// parse the storePath from the .narinfo
	storePath, err := storepath.FromAbsolutePath(narInfo.StorePath)
	if err != nil {
		return nil, fmt.Errorf("unable to parse StorePath: %w", err)
	}

	// construct the references, by parsing ReferenceNames and extracting the digest
	references := make([][]byte, len(narInfo.References))
	for i, referenceStr := range narInfo.References {
		// parse reference as store path
		referenceStorePath, err := storepath.FromString(referenceStr)
		if err != nil {
			return nil, fmt.Errorf("unable to parse reference %s as storepath: %w", referenceStr, err)
		}
		references[i] = referenceStorePath.Digest
	}

	// construct the narInfo.Signatures[*] from pathInfo.Narinfo.Signatures[*]
	narinfoSignatures := make([]*storev1pb.NARInfo_Signature, len(narInfo.Signatures))
	for i, narinfoSig := range narInfo.Signatures {
		narinfoSignatures[i] = &storev1pb.NARInfo_Signature{
			Name: narinfoSig.Name,
			Data: narinfoSig.Data,
		}
	}

	// assemble the PathInfo.
	pathInfo := &storev1pb.PathInfo{
		// embed a new root node with the name set to the store path basename.
		Node:       castorev1pb.RenamedNode(rootNode, storePath.String()),
		References: references,
		Narinfo: &storev1pb.NARInfo{
			NarSize:        narInfo.NarSize,
			NarSha256:      narInfo.FileHash.Digest(),
			Signatures:     narinfoSignatures,
			ReferenceNames: narInfo.References,
		},
	}

	// run Validate on the PathInfo, more as an additional sanity check our code is sound,
	// to make sure we populated everything properly, before returning it.
	// Fail hard if we fail validation, this is a code error.
	if _, err = pathInfo.Validate(); err != nil {
		panic(fmt.Sprintf("PathInfo failed validation: %v", err))
	}

	return pathInfo, nil

}
