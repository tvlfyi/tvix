map (e: (builtins.tryEval (builtins.toPath e)).success) [
  (builtins.throw "a")
  (./xyz + (builtins.throw "p"))
  # FIXME: test derivations and files.
]
