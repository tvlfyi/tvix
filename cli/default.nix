{ depot, pkgs, lib, ... }:

(depot.tvix.crates.workspaceMembers.tvix-cli.build.override {
  runTests = true;
}).overrideAttrs (_: {
  meta = {
    ci.extraSteps.eval-nixpkgs-stdenv = {
      label = ":nix: evaluate nixpkgs.stdenv in tvix";
      needsOutput = true;

      command = pkgs.writeShellScript "tvix-eval-stdenv" ''
        TVIX_OUTPUT=$(result/bin/tvix -E '(import ${pkgs.path} {}).stdenv.drvPath')
        EXPECTED='${/* the verbatim expected Tvix output: */ "=> \"${pkgs.stdenv.drvPath}\" :: string"}'

        echo "Tvix output: ''${TVIX_OUTPUT}"
        if [ "$TVIX_OUTPUT" != "$EXPECTED" ]; then
          echo "Correct would have been ''${EXPECTED}"
          exit 1
        fi

        echo "Output was correct."
      '';
    };
  };
})
