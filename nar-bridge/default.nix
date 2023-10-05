# Target containing just the proto files.

{ depot, pkgs, lib, ... }:

pkgs.buildGoModule {
  name = "nar-bridge";
  src = depot.third_party.gitignoreSource ./.;

  vendorHash = "sha256-DiGK6Lb+DA46zjJUZpkMSecF3cVst7KoGhcLG3OxtOc=";
}
