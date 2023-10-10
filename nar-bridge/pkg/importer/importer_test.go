package importer_test

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"testing"

	castorev1pb "code.tvl.fyi/tvix/castore/protos"
	"code.tvl.fyi/tvix/nar-bridge/pkg/importer"
	storev1pb "code.tvl.fyi/tvix/store/protos"
	"github.com/stretchr/testify/require"
)

func TestSymlink(t *testing.T) {
	f, err := os.Open("../../testdata/symlink.nar")
	require.NoError(t, err)

	actualPathInfo, err := importer.Import(
		context.Background(),
		f,
		func(blobReader io.Reader) ([]byte, error) {
			panic("no file contents expected!")
		}, func(directory *castorev1pb.Directory) ([]byte, error) {
			panic("no directories expected!")
		},
	)
	require.NoError(t, err)

	expectedPathInfo := &storev1pb.PathInfo{
		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_Symlink{
				Symlink: &castorev1pb.SymlinkNode{
					Name:   []byte(""),
					Target: []byte("/nix/store/somewhereelse"),
				},
			},
		},
		References: [][]byte{},
		Narinfo: &storev1pb.NARInfo{
			NarSize: 136,
			NarSha256: []byte{
				0x09, 0x7d, 0x39, 0x7e, 0x9b, 0x58, 0x26, 0x38, 0x4e, 0xaa, 0x16, 0xc4, 0x57, 0x71, 0x5d, 0x1c, 0x1a, 0x51, 0x67, 0x03, 0x13, 0xea, 0xd0, 0xf5, 0x85, 0x66, 0xe0, 0xb2, 0x32, 0x53, 0x9c, 0xf1,
			},
			Signatures:     []*storev1pb.NARInfo_Signature{},
			ReferenceNames: []string{},
		},
	}

	requireProtoEq(t, expectedPathInfo, actualPathInfo)
}

func TestRegular(t *testing.T) {
	f, err := os.Open("../../testdata/onebyteregular.nar")
	require.NoError(t, err)

	actualPathInfo, err := importer.Import(
		context.Background(),
		f,
		func(blobReader io.Reader) ([]byte, error) {
			contents, err := io.ReadAll(blobReader)
			require.NoError(t, err, "reading blobReader should not error")
			require.Equal(t, []byte{0x01}, contents, "contents read from blobReader should match expectations")
			return mustBlobDigest(bytes.NewBuffer(contents)), nil
		}, func(directory *castorev1pb.Directory) ([]byte, error) {
			panic("no directories expected!")
		},
	)
	require.NoError(t, err)

	// The blake3 digest of the 0x01 byte.
	BLAKE3_DIGEST_0X01 := []byte{
		0x48, 0xfc, 0x72, 0x1f, 0xbb, 0xc1, 0x72, 0xe0, 0x92, 0x5f, 0xa2, 0x7a, 0xf1, 0x67, 0x1d,
		0xe2, 0x25, 0xba, 0x92, 0x71, 0x34, 0x80, 0x29, 0x98, 0xb1, 0x0a, 0x15, 0x68, 0xa1, 0x88,
		0x65, 0x2b,
	}

	expectedPathInfo := &storev1pb.PathInfo{
		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_File{
				File: &castorev1pb.FileNode{
					Name:       []byte(""),
					Digest:     BLAKE3_DIGEST_0X01,
					Size:       1,
					Executable: false,
				},
			},
		},
		References: [][]byte{},
		Narinfo: &storev1pb.NARInfo{
			NarSize: 120,
			NarSha256: []byte{
				0x73, 0x08, 0x50, 0xa8, 0x11, 0x25, 0x9d, 0xbf, 0x3a, 0x68, 0xdc, 0x2e, 0xe8, 0x7a, 0x79, 0xaa, 0x6c, 0xae, 0x9f, 0x71, 0x37, 0x5e, 0xdf, 0x39, 0x6f, 0x9d, 0x7a, 0x91, 0xfb, 0xe9, 0x13, 0x4d,
			},
			Signatures:     []*storev1pb.NARInfo_Signature{},
			ReferenceNames: []string{},
		},
	}

	requireProtoEq(t, expectedPathInfo, actualPathInfo)
}

func TestEmptyDirectory(t *testing.T) {
	f, err := os.Open("../../testdata/emptydirectory.nar")
	require.NoError(t, err)

	expectedDirectory := &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{},
		Files:       []*castorev1pb.FileNode{},
		Symlinks:    []*castorev1pb.SymlinkNode{},
	}
	actualPathInfo, err := importer.Import(
		context.Background(),
		f,
		func(blobReader io.Reader) ([]byte, error) {
			panic("no file contents expected!")
		}, func(directory *castorev1pb.Directory) ([]byte, error) {
			requireProtoEq(t, expectedDirectory, directory)
			return mustDirectoryDigest(directory), nil
		},
	)
	require.NoError(t, err)

	expectedPathInfo := &storev1pb.PathInfo{
		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_Directory{
				Directory: &castorev1pb.DirectoryNode{
					Name:   []byte(""),
					Digest: mustDirectoryDigest(expectedDirectory),
					Size:   expectedDirectory.Size(),
				},
			},
		},
		References: [][]byte{},
		Narinfo: &storev1pb.NARInfo{
			NarSize: 96,
			NarSha256: []byte{
				0xa5, 0x0a, 0x5a, 0xb6, 0xd9, 0x92, 0xf5, 0x59, 0x8e, 0xdd, 0x92, 0x10, 0x50, 0x59, 0xfa, 0xe9, 0xac, 0xfc, 0x19, 0x29, 0x81, 0xe0, 0x8b, 0xd8, 0x85, 0x34, 0xc2, 0x16, 0x7e, 0x92, 0x52, 0x6a,
			},
			Signatures:     []*storev1pb.NARInfo_Signature{},
			ReferenceNames: []string{},
		},
	}
	requireProtoEq(t, expectedPathInfo, actualPathInfo)
}

func TestFull(t *testing.T) {
	f, err := os.Open("../../testdata/nar_1094wph9z4nwlgvsd53abfz8i117ykiv5dwnq9nnhz846s7xqd7d.nar")
	require.NoError(t, err)

	expectedDirectoryPaths := []string{
		"/bin",
		"/share/man/man1",
		"/share/man/man5",
		"/share/man/man8",
		"/share/man",
		"/share",
		"/",
	}
	expectedDirectories := make(map[string]*castorev1pb.Directory, len(expectedDirectoryPaths))

	// /bin is a leaf directory
	expectedDirectories["/bin"] = &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{},
		Files: []*castorev1pb.FileNode{
			{
				Name: []byte("arp"),
				Digest: []byte{
					0xfb, 0xc4, 0x61, 0x4a, 0x29, 0x27, 0x11, 0xcb, 0xcc, 0xe4, 0x99, 0x81, 0x9c, 0xf0, 0xa9, 0x17, 0xf7, 0xd0, 0x91, 0xbe, 0xea, 0x08, 0xcb, 0x5b, 0xaa, 0x76, 0x76, 0xf5, 0xee, 0x4f, 0x82, 0xbb,
				},
				Size:       55288,
				Executable: true,
			},
			{
				Name: []byte("hostname"),
				Digest: []byte{
					0x9c, 0x6a, 0xe4, 0xb5, 0xe4, 0x6c, 0xb5, 0x67, 0x45, 0x0e, 0xaa, 0x2a, 0xd8, 0xdd, 0x9b, 0x38, 0xd7, 0xed, 0x01, 0x02, 0x84, 0xf7, 0x26, 0xe1, 0xc7, 0xf3, 0x1c, 0xeb, 0xaa, 0x8a, 0x01, 0x30,
				},
				Size:       17704,
				Executable: true,
			},
			{
				Name: []byte("ifconfig"),
				Digest: []byte{
					0x25, 0xbe, 0x3b, 0x1d, 0xf4, 0x1a, 0x45, 0x42, 0x79, 0x09, 0x2c, 0x2a, 0x83, 0xf0, 0x0b, 0xff, 0xe8, 0xc0, 0x9c, 0x26, 0x98, 0x70, 0x15, 0x4d, 0xa8, 0xca, 0x05, 0xfe, 0x92, 0x68, 0x35, 0x2e,
				},
				Size:       72576,
				Executable: true,
			},
			{
				Name: []byte("nameif"),
				Digest: []byte{
					0x8e, 0xaa, 0xc5, 0xdb, 0x71, 0x08, 0x8e, 0xe5, 0xe6, 0x30, 0x1f, 0x2c, 0x3a, 0xf2, 0x42, 0x39, 0x0c, 0x57, 0x15, 0xaf, 0x50, 0xaa, 0x1c, 0xdf, 0x84, 0x22, 0x08, 0x77, 0x03, 0x54, 0x62, 0xb1,
				},
				Size:       18776,
				Executable: true,
			},
			{
				Name: []byte("netstat"),
				Digest: []byte{
					0x13, 0x34, 0x7e, 0xdd, 0x2a, 0x9a, 0x17, 0x0b, 0x3f, 0xc7, 0x0a, 0xe4, 0x92, 0x89, 0x25, 0x9f, 0xaa, 0xb5, 0x05, 0x6b, 0x24, 0xa7, 0x91, 0xeb, 0xaf, 0xf9, 0xe9, 0x35, 0x56, 0xaa, 0x2f, 0xb2,
				},
				Size:       131784,
				Executable: true,
			},
			{
				Name: []byte("plipconfig"),
				Digest: []byte{
					0x19, 0x7c, 0x80, 0xdc, 0x81, 0xdc, 0xb4, 0xc0, 0x45, 0xe1, 0xf9, 0x76, 0x51, 0x4f, 0x50, 0xbf, 0xa4, 0x69, 0x51, 0x9a, 0xd4, 0xa9, 0xe7, 0xaa, 0xe7, 0x0d, 0x53, 0x32, 0xff, 0x28, 0x40, 0x60,
				},
				Size:       13160,
				Executable: true,
			},
			{
				Name: []byte("rarp"),
				Digest: []byte{
					0x08, 0x85, 0xb4, 0x85, 0x03, 0x2b, 0x3c, 0x7a, 0x3e, 0x24, 0x4c, 0xf8, 0xcc, 0x45, 0x01, 0x9e, 0x79, 0x43, 0x8c, 0x6f, 0x5e, 0x32, 0x46, 0x54, 0xb6, 0x68, 0x91, 0x8e, 0xa0, 0xcb, 0x6e, 0x0d,
				},
				Size:       30384,
				Executable: true,
			},
			{
				Name: []byte("route"),
				Digest: []byte{
					0x4d, 0x14, 0x20, 0x89, 0x9e, 0x76, 0xf4, 0xe2, 0x92, 0x53, 0xee, 0x9b, 0x78, 0x7d, 0x23, 0x80, 0x6c, 0xff, 0xe6, 0x33, 0xdc, 0x4a, 0x10, 0x29, 0x39, 0x02, 0xa0, 0x60, 0xff, 0xe2, 0xbb, 0xd7,
				},
				Size:       61928,
				Executable: true,
			},
			{
				Name: []byte("slattach"),
				Digest: []byte{
					0xfb, 0x25, 0xc3, 0x73, 0xb7, 0xb1, 0x0b, 0x25, 0xcd, 0x7b, 0x62, 0xf6, 0x71, 0x83, 0xfe, 0x36, 0x80, 0xf6, 0x48, 0xc3, 0xdb, 0xd8, 0x0c, 0xfe, 0xb8, 0xd3, 0xda, 0x32, 0x9b, 0x47, 0x4b, 0x05,
				},
				Size:       35672,
				Executable: true,
			},
		},
		Symlinks: []*castorev1pb.SymlinkNode{
			{
				Name:   []byte("dnsdomainname"),
				Target: []byte("hostname"),
			},
			{
				Name:   []byte("domainname"),
				Target: []byte("hostname"),
			},
			{
				Name:   []byte("nisdomainname"),
				Target: []byte("hostname"),
			},
			{
				Name:   []byte("ypdomainname"),
				Target: []byte("hostname"),
			},
		},
	}

	// /share/man/man1 is a leaf directory.
	// The parser traversed over /sbin, but only added it to / which is still on the stack.
	expectedDirectories["/share/man/man1"] = &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{},
		Files: []*castorev1pb.FileNode{
			{
				Name: []byte("dnsdomainname.1.gz"),
				Digest: []byte{
					0x98, 0x8a, 0xbd, 0xfa, 0x64, 0xd5, 0xb9, 0x27, 0xfe, 0x37, 0x43, 0x56, 0xb3, 0x18, 0xc7, 0x2b, 0xcb, 0xe3, 0x17, 0x1c, 0x17, 0xf4, 0x17, 0xeb, 0x4a, 0xa4, 0x99, 0x64, 0x39, 0xca, 0x2d, 0xee,
				},
				Size:       40,
				Executable: false,
			},
			{
				Name: []byte("domainname.1.gz"),
				Digest: []byte{
					0x98, 0x8a, 0xbd, 0xfa, 0x64, 0xd5, 0xb9, 0x27, 0xfe, 0x37, 0x43, 0x56, 0xb3, 0x18, 0xc7, 0x2b, 0xcb, 0xe3, 0x17, 0x1c, 0x17, 0xf4, 0x17, 0xeb, 0x4a, 0xa4, 0x99, 0x64, 0x39, 0xca, 0x2d, 0xee,
				},
				Size:       40,
				Executable: false,
			},
			{
				Name: []byte("hostname.1.gz"),
				Digest: []byte{
					0xbf, 0x89, 0xe6, 0x28, 0x00, 0x24, 0x66, 0x79, 0x70, 0x04, 0x38, 0xd6, 0xdd, 0x9d, 0xf6, 0x0e, 0x0d, 0xee, 0x00, 0xf7, 0x64, 0x4f, 0x05, 0x08, 0x9d, 0xf0, 0x36, 0xde, 0x85, 0xf4, 0x75, 0xdb,
				},
				Size:       1660,
				Executable: false,
			},
			{
				Name: []byte("nisdomainname.1.gz"),
				Digest: []byte{
					0x98, 0x8a, 0xbd, 0xfa, 0x64, 0xd5, 0xb9, 0x27, 0xfe, 0x37, 0x43, 0x56, 0xb3, 0x18, 0xc7, 0x2b, 0xcb, 0xe3, 0x17, 0x1c, 0x17, 0xf4, 0x17, 0xeb, 0x4a, 0xa4, 0x99, 0x64, 0x39, 0xca, 0x2d, 0xee,
				},
				Size:       40,
				Executable: false,
			},
			{
				Name: []byte("ypdomainname.1.gz"),
				Digest: []byte{
					0x98, 0x8a, 0xbd, 0xfa, 0x64, 0xd5, 0xb9, 0x27, 0xfe, 0x37, 0x43, 0x56, 0xb3, 0x18, 0xc7, 0x2b, 0xcb, 0xe3, 0x17, 0x1c, 0x17, 0xf4, 0x17, 0xeb, 0x4a, 0xa4, 0x99, 0x64, 0x39, 0xca, 0x2d, 0xee,
				},
				Size:       40,
				Executable: false,
			},
		},
		Symlinks: []*castorev1pb.SymlinkNode{},
	}

	// /share/man/man5 is a leaf directory
	expectedDirectories["/share/man/man5"] = &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{},
		Files: []*castorev1pb.FileNode{
			{
				Name: []byte("ethers.5.gz"),
				Digest: []byte{
					0x42, 0x63, 0x8c, 0xc4, 0x18, 0x93, 0xcf, 0x60, 0xd6, 0xff, 0x43, 0xbc, 0x16, 0xb4, 0xfd, 0x22, 0xd2, 0xf2, 0x05, 0x0b, 0x52, 0xdc, 0x6a, 0x6b, 0xff, 0x34, 0xe2, 0x6a, 0x38, 0x3a, 0x07, 0xe3,
				},
				Size:       563,
				Executable: false,
			},
		},
		Symlinks: []*castorev1pb.SymlinkNode{},
	}

	// /share/man/man8 is a leaf directory
	expectedDirectories["/share/man/man8"] = &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{},
		Files: []*castorev1pb.FileNode{
			{
				Name: []byte("arp.8.gz"),
				Digest: []byte{
					0xf5, 0x35, 0x4e, 0xf5, 0xf6, 0x44, 0xf7, 0x52, 0x0f, 0x42, 0xa0, 0x26, 0x51, 0xd9, 0x89, 0xf9, 0x68, 0xf2, 0xef, 0xeb, 0xba, 0xe1, 0xf4, 0x55, 0x01, 0x57, 0x77, 0xb7, 0x68, 0x55, 0x92, 0xef,
				},
				Size:       2464,
				Executable: false,
			},
			{
				Name: []byte("ifconfig.8.gz"),
				Digest: []byte{
					0x18, 0x65, 0x25, 0x11, 0x32, 0xee, 0x77, 0x91, 0x35, 0x4c, 0x3c, 0x24, 0xdb, 0xaf, 0x66, 0xdb, 0xfc, 0x17, 0x7b, 0xba, 0xe1, 0x3d, 0x05, 0xd2, 0xca, 0x6e, 0x2c, 0xe4, 0xef, 0xb8, 0xa8, 0xbe,
				},
				Size:       3382,
				Executable: false,
			},
			{
				Name: []byte("nameif.8.gz"),
				Digest: []byte{
					0x73, 0xc1, 0x27, 0xe8, 0x3b, 0xa8, 0x49, 0xdc, 0x0e, 0xdf, 0x70, 0x5f, 0xaf, 0x06, 0x01, 0x2c, 0x62, 0xe9, 0x18, 0x67, 0x01, 0x94, 0x64, 0x26, 0xca, 0x95, 0x22, 0xc0, 0xdc, 0xe4, 0x42, 0xb6,
				},
				Size:       523,
				Executable: false,
			},
			{
				Name: []byte("netstat.8.gz"),
				Digest: []byte{
					0xc0, 0x86, 0x43, 0x4a, 0x43, 0x57, 0xaa, 0x84, 0xa7, 0x24, 0xa0, 0x7c, 0x65, 0x38, 0x46, 0x1c, 0xf2, 0x45, 0xa2, 0xef, 0x12, 0x44, 0x18, 0xba, 0x52, 0x56, 0xe9, 0x8e, 0x6a, 0x0f, 0x70, 0x63,
				},
				Size:       4284,
				Executable: false,
			},
			{
				Name: []byte("plipconfig.8.gz"),
				Digest: []byte{
					0x2a, 0xd9, 0x1d, 0xa8, 0x9e, 0x0d, 0x05, 0xd0, 0xb0, 0x49, 0xaa, 0x64, 0xba, 0x29, 0x28, 0xc6, 0x45, 0xe1, 0xbb, 0x5e, 0x72, 0x8d, 0x48, 0x7b, 0x09, 0x4f, 0x0a, 0x82, 0x1e, 0x26, 0x83, 0xab,
				},
				Size:       889,
				Executable: false,
			},
			{
				Name: []byte("rarp.8.gz"),
				Digest: []byte{
					0x3d, 0x51, 0xc1, 0xd0, 0x6a, 0x59, 0x1e, 0x6d, 0x9a, 0xf5, 0x06, 0xd2, 0xe7, 0x7d, 0x7d, 0xd0, 0x70, 0x3d, 0x84, 0x64, 0xc3, 0x7d, 0xfb, 0x10, 0x84, 0x3b, 0xe1, 0xa9, 0xdf, 0x46, 0xee, 0x9f,
				},
				Size:       1198,
				Executable: false,
			},
			{
				Name: []byte("route.8.gz"),
				Digest: []byte{
					0x2a, 0x5a, 0x4b, 0x4f, 0x91, 0xf2, 0x78, 0xe4, 0xa9, 0x25, 0xb2, 0x7f, 0xa7, 0x2a, 0xc0, 0x8a, 0x4a, 0x65, 0xc9, 0x5f, 0x07, 0xa0, 0x48, 0x44, 0xeb, 0x46, 0xf9, 0xc9, 0xe1, 0x17, 0x96, 0x21,
				},
				Size:       3525,
				Executable: false,
			},
			{
				Name: []byte("slattach.8.gz"),
				Digest: []byte{
					0x3f, 0x05, 0x6b, 0x20, 0xe1, 0xe4, 0xf0, 0xba, 0x16, 0x15, 0x66, 0x6b, 0x57, 0x96, 0xe9, 0x9d, 0x83, 0xa8, 0x20, 0xaf, 0x8a, 0xca, 0x16, 0x4d, 0xa2, 0x6d, 0x94, 0x8e, 0xca, 0x91, 0x8f, 0xd4,
				},
				Size:       1441,
				Executable: false,
			},
		},
		Symlinks: []*castorev1pb.SymlinkNode{},
	}

	// /share/man holds /share/man/man{1,5,8}.
	expectedDirectories["/share/man"] = &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{
			{
				Name:   []byte("man1"),
				Digest: mustDirectoryDigest(expectedDirectories["/share/man/man1"]),
				Size:   expectedDirectories["/share/man/man1"].Size(),
			},
			{
				Name:   []byte("man5"),
				Digest: mustDirectoryDigest(expectedDirectories["/share/man/man5"]),
				Size:   expectedDirectories["/share/man/man5"].Size(),
			},
			{
				Name:   []byte("man8"),
				Digest: mustDirectoryDigest(expectedDirectories["/share/man/man8"]),
				Size:   expectedDirectories["/share/man/man8"].Size(),
			},
		},
		Files:    []*castorev1pb.FileNode{},
		Symlinks: []*castorev1pb.SymlinkNode{},
	}

	// /share holds /share/man.
	expectedDirectories["/share"] = &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{
			{
				Name:   []byte("man"),
				Digest: mustDirectoryDigest(expectedDirectories["/share/man"]),
				Size:   expectedDirectories["/share/man"].Size(),
			},
		},
		Files:    []*castorev1pb.FileNode{},
		Symlinks: []*castorev1pb.SymlinkNode{},
	}

	// / holds /bin, /share, and a /sbin symlink.
	expectedDirectories["/"] = &castorev1pb.Directory{
		Directories: []*castorev1pb.DirectoryNode{
			{
				Name:   []byte("bin"),
				Digest: mustDirectoryDigest(expectedDirectories["/bin"]),
				Size:   expectedDirectories["/bin"].Size(),
			},
			{
				Name:   []byte("share"),
				Digest: mustDirectoryDigest(expectedDirectories["/share"]),
				Size:   expectedDirectories["/share"].Size(),
			},
		},
		Files: []*castorev1pb.FileNode{},
		Symlinks: []*castorev1pb.SymlinkNode{
			{
				Name:   []byte("sbin"),
				Target: []byte("bin"),
			},
		},
	}
	// assert we populated the two fixtures properly
	require.Equal(t, len(expectedDirectoryPaths), len(expectedDirectories))

	numDirectoriesReceived := 0

	actualPathInfo, err := importer.Import(
		context.Background(),
		f,
		func(blobReader io.Reader) ([]byte, error) {
			// Don't really bother reading and comparing the contents here,
			// We already verify the right digests are produced by comparing the
			// directoryCb calls, and TestRegular ensures the reader works.
			return mustBlobDigest(blobReader), nil
		}, func(directory *castorev1pb.Directory) ([]byte, error) {
			// use actualDirectoryOrder to look up the Directory object we expect at this specific invocation.
			currentDirectoryPath := expectedDirectoryPaths[numDirectoriesReceived]

			expectedDirectory, found := expectedDirectories[currentDirectoryPath]
			require.True(t, found, "must find the current directory")

			requireProtoEq(t, expectedDirectory, directory)

			numDirectoriesReceived += 1
			return mustDirectoryDigest(directory), nil
		},
	)
	require.NoError(t, err)

	expectedPathInfo := &storev1pb.PathInfo{
		Node: &castorev1pb.Node{
			Node: &castorev1pb.Node_Directory{
				Directory: &castorev1pb.DirectoryNode{
					Name:   []byte(""),
					Digest: mustDirectoryDigest(expectedDirectories["/"]),
					Size:   expectedDirectories["/"].Size(),
				},
			},
		},
		References: [][]byte{},
		Narinfo: &storev1pb.NARInfo{
			NarSize: 464152,
			NarSha256: []byte{
				0xc6, 0xe1, 0x55, 0xb3, 0x45, 0x6e, 0x30, 0xb7, 0x61, 0x22, 0x63, 0xec, 0x09, 0x50, 0x70, 0x81, 0x1c, 0xaf, 0x8a, 0xbf, 0xd5, 0x9f, 0xaa, 0x72, 0xab, 0x82, 0xa5, 0x92, 0xef, 0xde, 0xb2, 0x53,
			},
			Signatures:     []*storev1pb.NARInfo_Signature{},
			ReferenceNames: []string{},
		},
	}
	requireProtoEq(t, expectedPathInfo, actualPathInfo)
}

// TestCallbackErrors ensures that errors returned from the callback function
// bubble up to the importer process, and are not ignored.
func TestCallbackErrors(t *testing.T) {
	t.Run("callback blob", func(t *testing.T) {
		// Pick an example NAR with a regular file.
		f, err := os.Open("../../testdata/onebyteregular.nar")
		require.NoError(t, err)

		targetErr := errors.New("expected error")

		_, err = importer.Import(
			context.Background(),
			f,
			func(blobReader io.Reader) ([]byte, error) {
				return nil, targetErr
			}, func(directory *castorev1pb.Directory) ([]byte, error) {
				panic("no directories expected!")
			},
		)
		require.ErrorIs(t, err, targetErr)
	})
	t.Run("callback directory", func(t *testing.T) {
		// Pick an example NAR with a directory node
		f, err := os.Open("../../testdata/emptydirectory.nar")
		require.NoError(t, err)

		targetErr := errors.New("expected error")

		_, err = importer.Import(
			context.Background(),
			f,
			func(blobReader io.Reader) ([]byte, error) {
				panic("no file contents expected!")
			}, func(directory *castorev1pb.Directory) ([]byte, error) {
				return nil, targetErr
			},
		)
		require.ErrorIs(t, err, targetErr)
	})
}

// TestPopDirectories is a regression test that ensures we handle the directory
// stack properly.
//
// This test case looks like:
//
// / (dir)
// /test (dir)
// /test/tested (file)
// /tested (file)
//
// We used to have a bug where the second `tested` file would appear as if
// it was in the `/test` dir because it has that dir as a string prefix.
func TestPopDirectories(t *testing.T) {
	f, err := os.Open("../../testdata/popdirectories.nar")
	require.NoError(t, err)
	defer f.Close()

	_, err = importer.Import(
		context.Background(),
		f,
		func(blobReader io.Reader) ([]byte, error) { return mustBlobDigest(blobReader), nil },
		func(directory *castorev1pb.Directory) ([]byte, error) {
			require.NoError(t, directory.Validate(), "directory validation shouldn't error")
			return mustDirectoryDigest(directory), nil
		},
	)
	require.NoError(t, err)
}
