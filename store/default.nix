{ depot, pkgs, lib, ... }:

let
  protoRoot = depot.nix.sparseTree depot.path.origSrc [
    ./protos/castore.proto
    ./protos/pathinfo.proto
  ];
in
depot.third_party.naersk.buildPackage {
  src = depot.third_party.gitignoreSource ./.;
  # see https://github.com/nix-community/naersk/issues/169
  root = depot.tvix.naerskRootFor ./Cargo.toml;

  nativeBuildInputs = [ pkgs.protobuf ];

  PROTO_ROOT = protoRoot;

  doCheck = true;
}
