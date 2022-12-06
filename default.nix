# Nix helpers for projects under //tvix

{ pkgs, ... }:

{
  # Construct a sparse tree for naersk's `root` field, for
  # compatibility with the workspace-workaround (see top-level comment
  # in //tvix/Cargo.toml)
  naerskRootFor = cargoToml: pkgs.runCommand "sparse-tvix-root" { } ''
    mkdir $out
    cp -aT ${./Cargo.lock} $out/Cargo.lock
    cp -aT ${cargoToml} $out/Cargo.toml
  '';
}
