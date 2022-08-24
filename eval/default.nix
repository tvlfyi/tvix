{ depot, ... }:

depot.third_party.naersk.buildPackage {
  src = ./.;
  doCheck = true;
}
