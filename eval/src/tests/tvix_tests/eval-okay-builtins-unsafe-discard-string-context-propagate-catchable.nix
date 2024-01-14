map (e: (builtins.tryEval (builtins.unsafeDiscardStringContext e)).success) [
  (builtins.throw "a")
  # FIXME: test derivations with throwables.
]
