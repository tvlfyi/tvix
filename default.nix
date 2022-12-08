# Nix helpers for projects under //tvix
{ pkgs, ... }:

{
  # Load the crate2nix crate tree.
  crates = import ./Cargo.nix {
    inherit pkgs;
    nixpkgs = pkgs.path;
  };

  # Provide a shell for the combined dependencies of all Tvix Rust
  # projects. Note that as this is manually maintained it may be
  # lacking something, but it is required for some people's workflows.
  #
  # This shell can be entered with e.g. `mg shell //tvix:shell`.
  shell = pkgs.mkShell {
    name = "tvix-rust-dev-env";
    packages = [
      pkgs.buf-language-server
      pkgs.cargo
      pkgs.clippy
      pkgs.rust-analyzer
      pkgs.rustc
      pkgs.rustfmt
      pkgs.protobuf
    ];
  };

  meta.ci.targets = [ "shell" ];
}
