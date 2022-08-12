{ depot, pkgs, ... }:

depot.third_party.naersk.buildPackage {
  src = ./.;
  doDoc = false;
  # Tests invoke nix-store binary
  doCheck = false;
}
