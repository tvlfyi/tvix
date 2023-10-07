# This file is shell.nix in the tvix josh workspace,
# *and* used to provide the //tvix:shell attribute in a full depot checkout.
# Hence, it may not use depot as a toplevel argument.

{
  # This falls back to the tvix josh workspace-provided nixpkgs checkout.
  # In the case of depot, it's always set explicitly.
  pkgs ? (import ./nixpkgs {
    depotOverlays = false;
    depot.third_party.sources = import ./sources { };
  })
, ...
}:

let
  iconvDarwinDep = pkgs.lib.optionals pkgs.stdenv.isDarwin [ pkgs.libiconv ];
in
pkgs.mkShell {
  name = "tvix-rust-dev-env";
  packages = [
    pkgs.buf-language-server
    pkgs.cargo
    pkgs.cargo-machete
    pkgs.clippy
    pkgs.evans
    pkgs.fuse
    pkgs.pkg-config
    pkgs.rust-analyzer
    pkgs.rustc
    pkgs.rustfmt
    pkgs.protobuf
  ] ++ iconvDarwinDep;
}
