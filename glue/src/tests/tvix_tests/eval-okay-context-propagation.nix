# We test various propagation of contexts under other builtins here.
let
  drv = derivation {
    name = "fail";
    builder = "/bin/false";
    system = "x86_64-linux";
    outputs = [ "out" "foo" ];
  };
  other-drv = derivation {
    name = "other-fail";
    builder = "/bin/false";
    system = "x86_64-linux";
    outputs = [ "out" "bar" ];
  };
  a-path-drv = builtins.path {
    name = "a-path-drv";
    path = ./eval-okay-context-introspection.nix;
  };
  another-path-drv = builtins.filterSource (_: true) ./eval-okay-context-introspection.nix;

  # `substr` propagates context, we truncate to an empty string and concatenate to the target
  # to infect it with the context of `copied`.
  appendContextFrom = copied: target: (builtins.substring 0 0 copied) + target;

  path = "${./eval-okay-context-introspection.nix}";

  combo-path = "${path}${drv.outPath}${drv.foo.outPath}${drv.drvPath}";

  mergeContext = a: b:
    builtins.getContext a // builtins.getContext b;

  preserveContext = origin: result:
    builtins.getContext "${result}" == builtins.getContext "${origin}";

  preserveContexts = origins: result:
    let union = builtins.foldl' (x: y: x // y) { } (builtins.map (d: builtins.getContext "${d}") origins);
    in
    union == builtins.getContext "${result}";
in
[
  # `toFile` should produce context.
  (builtins.hasContext "${(builtins.toFile "myself" "${./eval-okay-context-introspection.nix}")}")
  # `derivation` should produce context.
  (builtins.hasContext "${drv}")
  # `builtins.path` / `builtins.filterSource` should produce context.
  (builtins.hasContext "${a-path-drv}")
  (builtins.hasContext "${another-path-drv}")
  # Low-level test to ensure that interpolation is working as expected.
  (builtins.length (builtins.attrNames (builtins.getContext "${drv}${other-drv}")) == 2)
  (builtins.getContext "${drv}${other-drv}" == mergeContext drv other-drv)
  # Those three next tests are extremely related.
  # To test interpolation, we need concatenation to be working and vice versa.
  # In addition, we need `builtins.substring` empty string propagation to attach context
  # in absence of `builtins.appendContext`.
  # The previous test should ensure that we don't test vacuous truths.
  # Substring preserves contexts.
  (preserveContext combo-path (builtins.substring 0 0 combo-path)) # <- FIXME: broken
  # Interpolation preserves contexts.
  (preserveContext "${drv}${other-drv}" (appendContextFrom drv other-drv))
  # Concatenation preserves contexts.
  (preserveContext "${drv}${other-drv}" (drv + other-drv))
  # Special case when Nix does not assert that the length argument is non-negative
  # when the starting index is ≥ than the string's length.
  # FIXME: those three are broken too, NON DETERMINISTIC!!!
  (preserveContext combo-path (builtins.substring 5 (-5) (builtins.substring 0 0 combo-path)))
  (preserveContext combo-path (toString combo-path))
  # No replacement should yield at least the same context.
  (preserveContext combo-path (builtins.replaceStrings [ ] [ ] combo-path))
  # This is an idempotent replacement, it should yield therefore to full preservation of the context.
  (preserveContext "${drv}${drv}" (builtins.replaceStrings [ "${drv}" ] [ "${drv}" ] "${drv}"))
  # There's no context here, so no context should appear from `drv`.
  (preserveContext "abc" (builtins.replaceStrings [ "${drv}" ] [ "${drv}" ] "abc"))
  # Context should appear by a successful replacement.
  (preserveContext "${drv}" (builtins.replaceStrings [ "a" ] [ "${drv}" ] "a"))
  # We test multiple successful replacements.
  (preserveContexts [ drv other-drv ] (builtins.replaceStrings [ "a" "b" ] [ "${drv}" "${other-drv}" ] "ab"))
  # We test *empty* string replacements.
  (preserveContext "${drv}" (builtins.replaceStrings [ "" ] [ "${drv}" ] "abc"))
  (preserveContext "${drv}" (builtins.replaceStrings [ "" ] [ "${drv}" ] ""))
  # There should be no context in a parsed derivation name.
  (!builtins.any builtins.hasContext (builtins.attrValues (builtins.parseDrvName "${drv.name}")))
  # Nix does not propagate contexts for `match`.
  (!builtins.any builtins.hasContext (builtins.match "(.*)" "${drv}"))
  # `dirOf` preserves contexts of non-paths.
  (preserveContext "${drv}" (builtins.dirOf "${drv}"))
  (preserveContext "abc" (builtins.dirOf "abc"))
  # `baseNameOf propagates context of argument
  (preserveContext "${drv}" (builtins.baseNameOf drv))
  (preserveContext "abc" (builtins.baseNameOf "abc"))
  # `concatStringsSep` preserves contexts of both arguments.
  (preserveContexts [ drv other-drv ] (builtins.concatStringsSep "${other-drv}" (map toString [ drv drv drv drv drv ])))
  (preserveContext drv (builtins.concatStringsSep "|" (map toString [ drv drv drv drv drv ])))
  (preserveContext other-drv (builtins.concatStringsSep "${other-drv}" [ "abc" "def" ]))
  # `attrNames` will never ever produce context.
  (preserveContext "abc" (toString (builtins.attrNames { a = { }; b = { }; c = { }; })))
  # `toJSON` preserves context of its inputs.
  (preserveContexts [ drv other-drv ] (builtins.toJSON {
    a = [ drv ];
    b = [ other-drv ];
  }))
  (preserveContexts [ drv other-drv ] (builtins.toJSON {
    a.deep = [ drv ];
    b = [ other-drv ];
  }))
  (preserveContexts [ drv other-drv ] (builtins.toJSON {
    a = "${drv}";
    b = [ other-drv ];
  }))
  (preserveContexts [ drv other-drv ] (builtins.toJSON {
    a.deep = "${drv}";
    b = [ other-drv ];
  }))
  (preserveContexts [ drv other-drv ] (builtins.toJSON {
    a = "${drv} ${other-drv}";
  }))
  (preserveContexts [ drv other-drv ] (builtins.toJSON {
    a.b.c.d.e.f = "${drv} ${other-drv}";
  }))
]
