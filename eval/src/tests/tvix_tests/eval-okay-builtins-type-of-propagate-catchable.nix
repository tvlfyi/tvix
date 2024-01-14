map (e: (builtins.tryEval (builtins.typeOf e)).success) [
  (builtins.throw "a")
  {
    a = builtins.throw "b";
  }
  [ (builtins.throw "c") ]
  (./xyz + (builtins.throw "p"))
  # FIXME: test derivations and files.
]
