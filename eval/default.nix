{ depot, pkgs, lib, ... }:

lib.fix (self: depot.third_party.naersk.buildPackage (lib.fix (naerskArgs: {
  src = depot.third_party.gitignoreSource ./.;
  # see https://github.com/nix-community/naersk/issues/169
  root = depot.nix.sparseTree ./. [ ./Cargo.lock ./Cargo.toml ];

  doCheck = true;

  # Tell the test suite where to find upstream nix, to compare eval results
  # against
  NIX_INSTANTIATE_BINARY_PATH = "${pkgs.nix}/bin/nix-instantiate";

  meta.ci.targets = builtins.attrNames self.passthru;

  copySources = [
    "builtin-macros"
  ];

  passthru.benchmarks = depot.third_party.naersk.buildPackage (naerskArgs // {
    name = "tvix-eval-benchmarks";

    doCheck = false;

    cargoBuildOptions = opts: opts ++ [ "--benches" ];

    copyBinsFilter = ''
      select(.reason == "compiler-artifact" and any(.target.kind[] == "bench"; .))
    '';

    passthru = { };
  });
}))
)
