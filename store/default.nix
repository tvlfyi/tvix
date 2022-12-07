{ depot, pkgs, ... }:

let
  protoRoot = depot.nix.sparseTree depot.path.origSrc [
    ./protos/castore.proto
    ./protos/pathinfo.proto
  ];

  protobufDep = prev: (prev.nativeBuildInputs or [ ]) ++ [ pkgs.protobuf ];
in
depot.tvix.crates.workspaceMembers.tvix-store.build.override {
  # Ensure protobuf dependencies are available.
  # TODO: figure out a way to embed this directly in the //tvix
  # crate2nix config.
  crateOverrides = {
    prost-build = prev: {
      nativeBuildInputs = protobufDep prev;
    };

    tvix-store = prev: {
      PROTO_ROOT = protoRoot;
      nativeBuildInputs = protobufDep prev;
    };
  };

  runTests = true;
}
