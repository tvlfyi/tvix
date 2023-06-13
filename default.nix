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
  packages = with pkgs; [
    buf-language-server
    cargo
    clippy
    evans
    fuse
    pkg-config
    protobuf
    rust-analyzer
    rustc
    rustfmt
  ];
}
