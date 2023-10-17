package storev1

import (
	"bytes"
	"crypto/sha256"
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
		// ensure the NarSha256 digest has the correct length.
		if len(narInfo.GetNarSha256()) != sha256.Size {
			return nil, fmt.Errorf("invalid number of bytes for NarSha256: expected %d, got %d", sha256.Size, len(narInfo.GetNarSha256()))
		}

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

		// If the Deriver field is populated, ensure it parses to a StorePath.
		// We can't check for it to *not* end with .drv, as the .drv files produced by
		// recursive Nix end with multiple .drv suffixes, and only one is popped when
		// converting to this field.
		if deriver := narInfo.GetDeriver(); deriver != nil {
			deriverStorePath := storepath.StorePath{
				Name:   string(deriver.GetName()),
				Digest: deriver.GetDigest(),
			}
			if err := deriverStorePath.Validate(); err != nil {
				return nil, fmt.Errorf("invalid deriver field: %w", err)
			}
		}
	}

	// ensure there is a (root) node present
	rootNode := p.GetNode()
	if rootNode == nil {
		return nil, fmt.Errorf("root node must be set")
	}

	if err := rootNode.Validate(); err != nil {
		return nil, fmt.Errorf("root node failed validation: %w", err)
	}

	// for all three node types, ensure the name properly parses to a store path.
	// This is a stricter check as the ones already performed in the rootNode.Validate() call.
	var rootNodeName []byte

	if node := rootNode.GetDirectory(); node != nil {
		rootNodeName = node.GetName()
	} else if node := rootNode.GetFile(); node != nil {
		rootNodeName = node.GetName()
	} else if node := rootNode.GetSymlink(); node != nil {
		rootNodeName = node.GetName()
	} else {
		// already caught by rootNode.Validate()
		panic("unreachable")
	}

	storePath, err := storepath.FromString(string(rootNodeName))
	if err != nil {
		return nil, fmt.Errorf("unable to parse root node name %s as StorePath: %w", rootNodeName, err)
	}

	return storePath, nil
}
