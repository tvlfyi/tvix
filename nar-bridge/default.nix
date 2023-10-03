# Target containing just the proto files.

{ depot, pkgs, lib, ... }:

pkgs.buildGoModule {
  name = "nar-bridge";
  src = depot.third_party.gitignoreSource ./.;

  vendorHash = "sha256-wEd3CBK7r28U77LpWc0UtbMlihkI7dEdy+ZWtJOBTSs=";
}
