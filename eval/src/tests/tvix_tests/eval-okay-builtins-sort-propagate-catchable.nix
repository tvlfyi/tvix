map (e: (builtins.tryEval e).success) [
  (builtins.sort (builtins.throw "a") [ "" ])
  (builtins.sort (x: y: true) (builtins.throw "b"))
  (builtins.sort (_: _: builtins.throw "x") [ "" ])
  (builtins.sort (_: _: builtins.throw "x") [ "" "" ])
]
