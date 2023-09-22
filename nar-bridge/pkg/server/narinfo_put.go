package server

import (
	"net/http"
	"path"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/go-chi/chi/v5"
	"github.com/nix-community/go-nix/pkg/narinfo"
	"github.com/nix-community/go-nix/pkg/nixbase32"
	"github.com/nix-community/go-nix/pkg/nixpath"
	"github.com/sirupsen/logrus"
	log "github.com/sirupsen/logrus"
)

func registerNarinfoPut(s *Server) {
	s.handler.Put("/{outputhash:^["+nixbase32.Alphabet+"]{32}}.narinfo", func(w http.ResponseWriter, r *http.Request) {
		defer r.Body.Close()

		ctx := r.Context()
		log := log.WithField("outputhash", chi.URLParamFromCtx(ctx, "outputhash"))

		// TODO: decide on merging behaviour.
		// Maybe it's fine to add if contents are the same, but more sigs can be added?
		// Right now, just replace a .narinfo for a path that already exists.

		// read and parse the .narinfo file
		narInfo, err := narinfo.Parse(r.Body)
		if err != nil {
			log.WithError(err).Error("unable to parse narinfo")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("unable to parse narinfo"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		log = log.WithFields(logrus.Fields{
			"narhash":     narInfo.NarHash.SRIString(),
			"output_path": narInfo.StorePath,
		})

		var pathInfo *storev1pb.PathInfo

		// look up the narHash in our temporary map
		s.narHashToPathInfoMu.Lock()
		pathInfo, found := s.narHashToPathInfo[narInfo.NarHash.SRIString()]
		s.narHashToPathInfoMu.Unlock()
		if !found {
			log.Error("unable to find referred NAR")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("unable to find referred NAR"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		// compare fields with what we computed while receiving the NAR file

		// NarSize needs to match
		if pathInfo.Narinfo.NarSize != narInfo.NarSize {
			log.Error("narsize mismatch")
			w.WriteHeader(http.StatusBadRequest)
			_, err := w.Write([]byte("unable to parse narinfo"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}
		// We know the narhash in the .narinfo matches one of the two narhashes in the partial pathInfo,
		// because that's how we found it.

		// FUTUREWORK: We can't compare References yet, but it'd be a good idea to
		// do reference checking on .nar files server-side during upload.
		// We however still need to be parse them, because we store
		// the bytes in pathInfo.References, and the full strings in pathInfo.Narinfo.ReferenceNames.
		referencesBytes := make([][]byte, 0)
		for _, reference := range narInfo.References {
			np, err := nixpath.FromString(path.Join(nixpath.StoreDir, reference))
			if err != nil {
				log.WithField("reference", reference).WithError(err).Error("unable to parse reference")
				w.WriteHeader(http.StatusBadRequest)
				_, err := w.Write([]byte("unable to parse reference"))
				if err != nil {
					log.WithError(err).Errorf("unable to write error message to client")
				}

				return
			}
			referencesBytes = append(referencesBytes, np.Digest)
		}

		// assemble the []*storev1pb.NARInfo_Signature{} from narinfo.Signatures.
		pbNarinfoSignatures := make([]*storev1pb.NARInfo_Signature, 0)
		for _, narinfoSig := range narInfo.Signatures {

			pbNarinfoSignatures = append(pbNarinfoSignatures, &storev1pb.NARInfo_Signature{
				Name: narinfoSig.Name,
				Data: narinfoSig.Data,
			})
		}

		// If everything matches, We will add References, NAR signatures and the
		// output path name, and then upload to the pathinfo service.
		// We want a copy here, because we don't want to mutate the contents in the lookup table
		// until we get things back from the remote store.
		pathInfoToUpload := &storev1pb.PathInfo{
			Node:       nil, // set below
			References: referencesBytes,
			Narinfo: &storev1pb.NARInfo{
				NarSize:        pathInfo.Narinfo.NarSize,
				NarSha256:      pathInfo.Narinfo.NarSha256,
				Signatures:     pbNarinfoSignatures,
				ReferenceNames: narInfo.References,
			},
		}

		// We need to add the basename of the storepath from the .narinfo
		// to the pathInfo to be sent.
		switch v := (pathInfo.GetNode().GetNode()).(type) {
		case *castorev1pb.Node_File:
			pathInfoToUpload.Node = &castorev1pb.Node{
				Node: &castorev1pb.Node_File{
					File: &castorev1pb.FileNode{
						Name:       []byte(path.Base(narInfo.StorePath)),
						Digest:     v.File.Digest,
						Size:       v.File.Size,
						Executable: v.File.Executable,
					},
				},
			}
		case *castorev1pb.Node_Symlink:
			pathInfoToUpload.Node = &castorev1pb.Node{
				Node: &castorev1pb.Node_Symlink{
					Symlink: &castorev1pb.SymlinkNode{
						Name:   []byte(path.Base(narInfo.StorePath)),
						Target: v.Symlink.Target,
					},
				},
			}
		case *castorev1pb.Node_Directory:
			pathInfoToUpload.Node = &castorev1pb.Node{
				Node: &castorev1pb.Node_Directory{
					Directory: &castorev1pb.DirectoryNode{
						Name:   []byte(path.Base(narInfo.StorePath)),
						Digest: v.Directory.Digest,
						Size:   v.Directory.Size,
					},
				},
			}
		}

		receivedPathInfo, err := s.pathInfoServiceClient.Put(ctx, pathInfoToUpload)
		if err != nil {
			log.WithError(err).Error("unable to upload pathinfo to service")
			w.WriteHeader(http.StatusInternalServerError)
			_, err := w.Write([]byte("unable to upload pathinfo to server"))
			if err != nil {
				log.WithError(err).Errorf("unable to write error message to client")
			}

			return
		}

		log.Debugf("received new pathInfo: %v+", receivedPathInfo)

		// TODO: update the local temporary pathinfo with this?
	})
}
