# Target containing just the proto files.

{ depot, pkgs, lib, ... }:

pkgs.buildGoModule {
  name = "nar-bridge";
  src = depot.third_party.gitignoreSource ./.;

  vendorHash = "sha256-PlmFiGQPHqc+4JG0HAWW+oj8ruL7PTmGNJe20ansrxg=";
}
