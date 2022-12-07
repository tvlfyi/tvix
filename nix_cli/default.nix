{ depot, ... }:

depot.tvix.crates.workspaceMembers.nix-cli.build.override {
  runTests = true;
}
