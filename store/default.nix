{ depot, pkgs, ... }:

let
  mkImportCheck = p: expectedPath: {
    label = ":nix :import ${p} with tvix-store import";
    needsOutput = true;
    command = pkgs.writeShellScript "tvix-import-check" ''
      export BLOB_SERVICE_ADDR=memory://
      export DIRECTORY_SERVICE_ADDR=memory://
      export PATH_INFO_SERVICE_ADDR=memory://
      TVIX_STORE_OUTPUT=$(result/bin/tvix-store import ${p})
      EXPECTED='${/* the vebatim expected Tvix output: */expectedPath}'

      echo "tvix-store output: ''${TVIX_STORE_OUTPUT}"
      if [ "$TVIX_STORE_OUTPUT" != "$EXPECTED" ]; then
        echo "Correct would have been ''${EXPECTED}"
        exit 1
      fi

      echo "Output was correct."
    '';
  };
in

(depot.tvix.crates.workspaceMembers.tvix-store.build.override {
  runTests = true;
}).overrideAttrs (_: {
  meta.ci.extraSteps = {
    import-docs = (mkImportCheck "tvix/store/docs" ./docs);
  };
})
