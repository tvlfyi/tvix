map (e: (builtins.tryEval e).success) [
  (builtins.stringLength (builtins.throw "a"))
  # FIXME(raitobezarius): test coercions too.
]
