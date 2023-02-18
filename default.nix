# Externally importable TVL depot stack. This is intended to be called
# with a supplied package set, otherwise the package set currently in
# use by the TVL depot will be used.
#
{ pkgs ? (import ./nixpkgs {
    depotOverlays = false;
    depot.third_party.sources = import ./sources { };
  })
, ...
}:

pkgs.mkShell {
  name = "tvix-rust-dev-env";
  packages = [
    pkgs.buf-language-server
    pkgs.cargo
    pkgs.clippy
    pkgs.rust-analyzer
    pkgs.rustc
    pkgs.rustfmt
    pkgs.protobuf
    pkgs.evans
  ];
}
