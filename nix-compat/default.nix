{ depot, ... }:

depot.tvix.crates.workspaceMembers.nix-compat.build.override {
  runTests = true;
}
