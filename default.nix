# Nix helpers for projects under //tvix
{ pkgs, ... }:

{
  # Load the crate2nix crate tree.
  crates = import ./Cargo.nix {
    inherit pkgs;
    nixpkgs = pkgs.path;
  };
}
