{ lib, depot, ... }:

{
  mkFeaturePowerset = { crateName, features, override ? { } }:
    let
      powerset = xs:
        let
          addElement = set: element:
            set ++ map (e: [ element ] ++ e) set;
        in
        lib.foldl' addElement [ [ ] ] xs;
    in
    lib.listToAttrs (map
      (featuresPowerset: {
        name = if featuresPowerset != [ ] then "with-features-${lib.concatStringsSep "-" featuresPowerset}" else "no-features";
        value = depot.tvix.crates.workspaceMembers.${crateName}.build.override (old: {
          runTests = true;
          features = featuresPowerset;
        } // (if lib.isFunction override then override old else override)
        );
      })
      (powerset features));

  # Filters the given source, only keeping files related to the build, preventing unnecessary rebuilds.
  # Includes src in the root, all other .rs files and optionally Cargo specific files.
  # Additional files to be included can be specified in extraFileset.
  filterRustCrateSrc =
    { root # The original src
    , extraFileset ? null # Additional filesets to include (e.g. fileFilter for proto files)
    , cargoSupport ? false
    }:
    lib.fileset.toSource {
      inherit root;
      fileset = lib.fileset.intersection
        (lib.fileset.fromSource root) # We build our final fileset from the original src
        (lib.fileset.unions ([
          (root + "/src")
          (lib.fileset.fileFilter (f: f.hasExt "rs") root)
        ] ++ lib.optionals cargoSupport [
          (lib.fileset.fileFilter (f: f.name == "Cargo.toml") root)
          (lib.fileset.maybeMissing (root + "/Cargo.lock"))
        ] ++ lib.optional (extraFileset != null) extraFileset));
    };
}
