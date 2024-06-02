# Contrary to the Nix tests, this one does not make any use of `builtins.appendContext`
# It's a weaker yet interesting test by abusing knowledge on how does our builtins
# performs propagation.
let
  drv = derivation {
    name = "fail";
    builder = "/bin/false";
    system = "x86_64-linux";
    outputs = [ "out" "foo" ];
  };

  # `substr` propagates context, we truncate to an empty string and concatenate to the target
  # to infect it with the context of `copied`.
  appendContextFrom = copied: target: (builtins.substring 0 0 "${copied}") + "${target}";

  # `split` discards (!!) contexts, we first split by `/` (there's at least one such `/` by
  # virtue of `target` being a store path, i.e. starting with `$store_root/$derivation_name`)
  # then, we reassemble the list into a proper string.
  discardContext = target: builtins.concatStringsSep "" (builtins.split "(.*)" "${target}");

  # Note that this should never return true for any attribute set.
  hasContextInAttrKeys = attrs: builtins.any builtins.hasContext (builtins.attrNames attrs);

  path = "${./eval-okay-context-introspection.nix}";

  # This is a context-less attribute set, which should be exactly the same
  # as `builtins.getContext combo-path`.
  desired-context = {
    "${builtins.unsafeDiscardStringContext path}" = {
      path = true;
    };
    "${builtins.unsafeDiscardStringContext drv.drvPath}" = {
      outputs = [ "foo" "out" ];
      allOutputs = true;
    };
  };

  combo-path = "${path}${drv.outPath}${drv.foo.outPath}${drv.drvPath}";
  legit-context = builtins.getContext combo-path;

  reconstructed-path = appendContextFrom combo-path
    (builtins.unsafeDiscardStringContext combo-path);

  an-str = {
    a = "${drv}";
  };
  an-list = {
    b = [ drv ];
  };

  # Eta rule for strings with context.
  etaRule = str:
    str == appendContextFrom
      str
      (builtins.unsafeDiscardStringContext str);

  etaRule' = str:
    str == appendContextFrom
      str
      (discardContext str);

in
[
  (!hasContextInAttrKeys desired-context)
  (legit-context."${builtins.unsafeDiscardStringContext path}".path)
  (legit-context."${builtins.unsafeDiscardStringContext drv.drvPath}".outputs == [ "foo" "out" ])
  # `allOutputs` is present only on DrvClosure-style context string, i.e. the
  # context string of a drvPath itself, not an outPath.
  (!builtins.hasAttr "allOutputs" (builtins.getContext drv.outPath)."${builtins.unsafeDiscardStringContext drv.drvPath}")
  (builtins.hasAttr "allOutputs" legit-context."${builtins.unsafeDiscardStringContext drv.drvPath}")
  (builtins.hasAttr "allOutputs" (builtins.getContext drv.drvPath)."${builtins.unsafeDiscardStringContext drv.drvPath}")
  (legit-context == desired-context) # FIXME(raitobezarius): this should not use `builtins.seq`, this is a consequence of excessive laziness of Tvix, I believe.
  (reconstructed-path == combo-path)
  # These still fail with an internal error
  # (etaRule' "foo")
  # (etaRule' combo-path)
  (etaRule "foo")
  (etaRule drv.drvPath)
  (etaRule drv.foo.outPath)
  # `toJSON` tests
  (builtins.hasContext (builtins.toJSON an-str))
  (builtins.hasContext (builtins.toJSON an-list))
]
