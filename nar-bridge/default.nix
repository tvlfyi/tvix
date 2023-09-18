# Target containing just the proto files.

{ depot, pkgs, lib, ... }:

pkgs.buildGoModule {
  name = "nar-bridge";
  src = depot.third_party.gitignoreSource ./.;

  vendorHash = "sha256-xaNf/bnSuQpt1vadFmYt4NcpJQD3KmiYQ4SrdtiK33U=";
}
