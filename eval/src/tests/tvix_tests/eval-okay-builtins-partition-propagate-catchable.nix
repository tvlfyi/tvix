map (e: (builtins.tryEval e).success) [
  (builtins.partition (builtins.throw "a") [ "" ])
  (builtins.partition (x: true) (builtins.throw "b"))
  (builtins.partition (_: builtins.throw "x") [ "" ])
]
