map (e: (builtins.tryEval e).success) [
  (builtins.genList (builtins.throw "a") 10)
  (builtins.genList (i: "") (builtins.throw "b"))
  (builtins.genList (i: builtins.throw "x") 5)
]
