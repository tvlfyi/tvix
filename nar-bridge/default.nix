# Target containing just the proto files.

{ depot, pkgs, lib, ... }:

pkgs.buildGoModule {
  name = "nar-bridge";
  src = depot.third_party.gitignoreSource ./.;

  vendorHash = "sha256-ankJbu6mHziF04NTA8opnWH765Jv1wQALYI8SeEst1Q=";
}
