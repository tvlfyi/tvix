{ depot, ... }:

depot.tvix.crates.workspaceMembers.tvix-store.build.override {
  runTests = true;
}
