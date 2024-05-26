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
}
