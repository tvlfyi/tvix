{ depot, ... }:

depot.tvix.crates.workspaceMembers.tvix-serde.build.override {
  runTests = true;
}
