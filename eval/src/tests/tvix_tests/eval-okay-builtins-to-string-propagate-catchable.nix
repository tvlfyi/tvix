map (e: (builtins.tryEval (builtins.toString e)).success) [
  (builtins.throw "a")
  [ (builtins.throw "a") ]
  [ "abc" (builtins.throw "a") ]
  "abc${builtins.throw "c"}"
  # FIXME: test derivations and files.
]
