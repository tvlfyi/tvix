{ depot, ... }:

depot.tvix.crates.workspaceMembers.tvix-nar.build.override {
  runTests = true;
}
