let
  depot = (import ./.. { });
  pkgs = depot.third_party.nixpkgs;

in
pkgs.mkShell {
  buildInputs = [
    pkgs.rustup
    pkgs.rust-analyzer
  ];
}
