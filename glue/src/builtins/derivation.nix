# LGPL-2.1-or-later
#
# taken from: https://github.com/NixOS/nix/blob/master/src/libexpr/primops/derivation.nix
#
# TODO: rewrite in native Rust code

/* This is the implementation of the ‘derivation’ builtin function.
   It's actually a wrapper around the ‘derivationStrict’ primop. */

drvAttrs @ { outputs ? [ "out" ], ... }:

let

  strict = derivationStrict drvAttrs;

  commonAttrs = drvAttrs // (builtins.listToAttrs outputsList) //
    {
      all = map (x: x.value) outputsList;
      inherit drvAttrs;
    };

  outputToAttrListElement = outputName:
    {
      name = outputName;
      value = commonAttrs // {
        outPath = builtins.getAttr outputName strict;
        drvPath = strict.drvPath;
        type = "derivation";
        inherit outputName;
      };
    };

  outputsList = map outputToAttrListElement outputs;

in
(builtins.head outputsList).value
