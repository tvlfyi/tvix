map (e: (builtins.tryEval e).success) [
  (builtins.functionArgs (_: builtins.throw "a"))
  (builtins.functionArgs (builtins.throw "b"))
]
