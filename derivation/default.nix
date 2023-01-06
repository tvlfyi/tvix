{ depot, ... }:

depot.tvix.crates.workspaceMembers.tvix-derivation.build.override {
  runTests = true;
}
