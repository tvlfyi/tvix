{ depot, pkgs, lib, ... }:

let
  mkNixpkgsEvalCheck = attrset: expectedPath: {
    label = ":nix: evaluate nixpkgs.${attrset} in tvix";
    needsOutput = true;

    command = pkgs.writeShellScript "tvix-eval-${builtins.replaceStrings [".drv"] ["-drv"] attrset}" ''
      TVIX_OUTPUT=$(result/bin/tvix -E '(import ${pkgs.path} {}).${attrset}')
      EXPECTED='${/* the verbatim expected Tvix output: */ "=> \"${expectedPath}\" :: string"}'

      echo "Tvix output: ''${TVIX_OUTPUT}"
      if [ "$TVIX_OUTPUT" != "$EXPECTED" ]; then
        echo "Correct would have been ''${EXPECTED}"
        exit 1
      fi

      echo "Output was correct."
    '';
  };
in

(depot.tvix.crates.workspaceMembers.tvix-cli.build.override {
  runTests = true;
}).overrideAttrs (_: {
  meta = {
    ci.extraSteps = {
      eval-nixpkgs-stdenv-drvpath = (mkNixpkgsEvalCheck "stdenv.drvPath" pkgs.stdenv.drvPath);
      eval-nixpkgs-stdenv-outpath = (mkNixpkgsEvalCheck "stdenv.outPath" pkgs.stdenv.outPath);
      eval-nixpkgs-hello-outpath = (mkNixpkgsEvalCheck "hello.outPath" pkgs.hello.outPath);

      # This is the furthest we get starting with stdenv we hit something similar to b/261
      eval-nixpkgs-cross-gcc-outpath = (mkNixpkgsEvalCheck "pkgsCross.aarch64-multiplatform.buildPackages.gcc.outPath" pkgs.pkgsCross.aarch64-multiplatform.buildPackages.gcc.outPath);
    };
  };
})
