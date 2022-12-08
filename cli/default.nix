{ depot, pkgs, lib, ... }:

depot.tvix.crates.workspaceMembers.tvix-cli.build.override {
  runTests = true;
}
