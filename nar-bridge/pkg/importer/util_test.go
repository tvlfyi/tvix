package importer_test

import (
	"io"
	"testing"

	castorev1pb "code.tvl.fyi/tvix/castore-go"
	"github.com/google/go-cmp/cmp"
	"google.golang.org/protobuf/testing/protocmp"
	"lukechampine.com/blake3"
)

func requireProtoEq(t *testing.T, expected interface{}, actual interface{}) {
	if diff := cmp.Diff(expected, actual, protocmp.Transform()); diff != "" {
		t.Errorf("unexpected difference:\n%v", diff)
	}
}

func mustDirectoryDigest(d *castorev1pb.Directory) []byte {
	dgst, err := d.Digest()
	if err != nil {
		panic(err)
	}
	return dgst
}

func mustBlobDigest(r io.Reader) []byte {
	hasher := blake3.New(32, nil)
	_, err := io.Copy(hasher, r)
	if err != nil {
		panic(err)
	}
	return hasher.Sum([]byte{})
}
