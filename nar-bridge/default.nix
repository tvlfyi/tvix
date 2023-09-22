# Target containing just the proto files.

{ depot, pkgs, lib, ... }:

pkgs.buildGoModule {
  name = "nar-bridge";
  src = depot.third_party.gitignoreSource ./.;

  vendorHash = "sha256-sxyoVyHq/FJbNzVa6Xlnv0+/vxLxf0SAY4ZbvRySoFQ=";
}
