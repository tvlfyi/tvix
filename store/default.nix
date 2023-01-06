{ depot, ... }:

depot.tvix.crates.workspaceMembers.tvix-store-bin.build.override {
  runTests = true;
}
