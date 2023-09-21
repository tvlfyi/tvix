{ depot, pkgs, ... }:

depot.tvix.crates.workspaceMembers.tvix-castore.build.override {
  runTests = true;
}
