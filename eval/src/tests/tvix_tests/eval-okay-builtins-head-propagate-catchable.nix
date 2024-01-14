map (e: (builtins.tryEval e).success) [
  (builtins.head (builtins.throw "a"))
  (builtins.head [ (builtins.throw "a") ])
]
