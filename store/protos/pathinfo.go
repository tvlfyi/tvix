package storev1

import (
	"bytes"
	"encoding/base64"
	"fmt"

	"github.com/nix-community/go-nix/pkg/storepath"
)

// Validate performs some checks on the PathInfo struct, returning either the
// StorePath of the root node, or an error.
func (p *PathInfo) Validate() (*storepath.StorePath, error) {
	// ensure References has the right number of bytes.
	for i, reference := range p.GetReferences() {
		if len(reference) != storepath.PathHashSize {
			return nil, fmt.Errorf("invalid length of digest at position %d, expected %d, got %d", i, storepath.PathHashSize, len(reference))
		}
	}

	// If there's a Narinfo field populated..
	if narInfo := p.GetNarinfo(); narInfo != nil {
		// ensure the number of references matches len(References).
		if len(narInfo.GetReferenceNames()) != len(p.GetReferences()) {
			return nil, fmt.Errorf("inconsistent number of references: %d (references) vs %d (narinfo)", len(narInfo.GetReferenceNames()), len(p.GetReferences()))
		}

		// for each ReferenceName…
		for i, referenceName := range narInfo.GetReferenceNames() {
			// ensure it parses to a store path
			storePath, err := storepath.FromString(referenceName)
			if err != nil {
				return nil, fmt.Errorf("invalid ReferenceName at position %d: %w", i, err)
			}

			// ensure the digest matches the one at References[i]
			if !bytes.Equal(p.GetReferences()[i], storePath.Digest) {
				return nil, fmt.Errorf(
					"digest in ReferenceName at position %d does not match digest in PathInfo, expected %s, got %s",
					i,
					base64.StdEncoding.EncodeToString(p.GetReferences()[i]),
					base64.StdEncoding.EncodeToString(storePath.Digest),
				)
			}
		}
	}

	// ensure there is a (root) node present
	rootNode := p.GetNode()
	if rootNode == nil {
		return nil, fmt.Errorf("root node must be set")
	}

	// for all three node types, ensure the name properly parses to a store path,
	// and in case it refers to a digest, ensure it has the right length.

	if node := rootNode.GetDirectory(); node != nil {
		if len(node.Digest) != 32 {
			return nil, fmt.Errorf("invalid digest size for %s, expected %d, got %d", node.Name, 32, len(node.Digest))
		}

		storePath, err := storepath.FromString(string(node.GetName()))

		if err != nil {
			return nil, fmt.Errorf("unable to parse %s as StorePath: %w", node.Name, err)
		}

		return storePath, nil

	} else if node := rootNode.GetFile(); node != nil {
		if len(node.Digest) != 32 {
			return nil, fmt.Errorf("invalid digest size for %s, expected %d, got %d", node.Name, 32, len(node.Digest))
		}

		storePath, err := storepath.FromString(string(node.GetName()))
		if err != nil {
			return nil, fmt.Errorf("unable to parse %s as StorePath: %w", node.Name, err)
		}

		return storePath, nil

	} else if node := rootNode.GetSymlink(); node != nil {
		storePath, err := storepath.FromString(string(node.GetName()))

		if err != nil {
			return nil, fmt.Errorf("unable to parse %s as StorePath: %w", node.Name, err)
		}

		return storePath, nil

	} else {
		// this would only happen if we introduced a new type
		panic("unreachable")
	}
}
