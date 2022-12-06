{ depot, pkgs, ... }:

depot.third_party.naersk.buildPackage {
  src = ./.;
  root = depot.tvix.naerskRootFor ./Cargo.toml;
  doDoc = false;
  doCheck = true;
}
