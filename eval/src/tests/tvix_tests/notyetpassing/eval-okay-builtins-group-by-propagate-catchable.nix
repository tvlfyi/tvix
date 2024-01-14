map (e: (builtins.tryEval e).success) [
  (builtins.groupBy (builtins.throw "a") [ "" ])
  (builtins.groupBy (x: true) (builtins.throw "b"))
  (builtins.groupBy (_: builtins.throw "x") [ "" ])
]
