map (e: (builtins.tryEval e).success) [
  (builtins.tail (builtins.throw "a"))
  (builtins.tail [ (builtins.throw "a") ])
  (builtins.tail [ (builtins.throw "a") "a" ])
  (builtins.tail [ (builtins.throw "a") (builtins.throw "a") ])
]
