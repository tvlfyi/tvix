map (e: (builtins.tryEval e).success) [
  (builtins.map (builtins.throw "a") [ "" ])
  (builtins.map (x: true) (builtins.throw "b"))
  (builtins.map (_: builtins.throw "x") [ "" ])
]
