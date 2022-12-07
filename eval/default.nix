# TODO: find a way to build the benchmarks via crate2nix
{ depot, pkgs, ... }:

depot.tvix.crates.workspaceMembers.tvix-eval.build.override {
  runTests = true;

  # Make C++ Nix available, to compare eval results against.
  testInputs = [ pkgs.nix ];
}
