# This tests verifies that the Nix implementation evaluating this has at least
# all the builtins given in `minimalBuiltins`. We don't test a precise list of
# builtins since we accept that there will always be difference between the
# builtins sets of Tvix, C++ Nix 2.3 and newer C++ Nix versions, as new builtins
# are added.
#
# Tvix also may choose never to implement some builtins if they are only useful
# for flakes or perform well enough via the shims nixpkgs usually provides.

let
  # C++ Nix 2.3 builtins except valueSize which is removed in later versions
  minimalBuiltins = [
    "abort" "add" "addErrorContext" "all" "any" "appendContext" "attrNames"
    "attrValues" "baseNameOf" "bitAnd" "bitOr" "bitXor" "builtins" "catAttrs"
    "compareVersions" "concatLists" "concatMap" "concatStringsSep"
    "currentSystem" "currentTime" "deepSeq" "derivation" "derivationStrict"
    "dirOf" "div" "elem" "elemAt" "false" "fetchGit" "fetchMercurial"
    "fetchTarball" "fetchurl" "filter" "filterSource" "findFile" "foldl'"
    "fromJSON" "fromTOML" "functionArgs" "genList" "genericClosure" "getAttr"
    "getContext" "getEnv" "hasAttr" "hasContext" "hashFile" "hashString" "head"
    "import" "intersectAttrs" "isAttrs" "isBool" "isFloat" "isFunction" "isInt"
    "isList" "isNull" "isPath" "isString" "langVersion" "length" "lessThan"
    "listToAttrs" "map" "mapAttrs" "match" "mul" "nixPath" "nixVersion" "null"
    "parseDrvName" "partition" "path" "pathExists" "placeholder" "readDir"
    "readFile" "removeAttrs" "replaceStrings" "scopedImport" "seq" "sort"
    "split" "splitVersion" "storeDir" "storePath" "stringLength" "sub"
    "substring" "tail" "throw" "toFile" "toJSON" "toPath" "toString" "toXML"
    "trace" "true" "tryEval" "typeOf" "unsafeDiscardOutputDependency"
    "unsafeDiscardStringContext" "unsafeGetAttrPos"
  ];

  intersectLists = as: bs: builtins.filter (a: builtins.elem a bs) as;
in

intersectLists minimalBuiltins (builtins.attrNames builtins)
