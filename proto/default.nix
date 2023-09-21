# Target containing just the proto files used in tvix

{ depot, lib, ... }:

depot.nix.sparseTree {
  name = "tvix-protos";
  root = depot.path.origSrc;
  paths = [
    ../castore/protos/castore.proto
    ../castore/protos/rpc_blobstore.proto
    ../castore/protos/rpc_directory.proto
    ../store/protos/pathinfo.proto
    ../store/protos/rpc_pathinfo.proto
  ];
}
