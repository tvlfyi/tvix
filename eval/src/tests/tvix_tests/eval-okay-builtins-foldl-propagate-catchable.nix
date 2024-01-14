map (e: (builtins.tryEval e).success) [
  (builtins.foldl' (builtins.throw "a") {} [ {} {} {} ])
  (builtins.foldl' (x: y: x // y) {} (builtins.throw "b"))
  (builtins.foldl' (_: _: builtins.throw "x") {} [ {} ])
  (builtins.foldl' (x: y: x // y) (builtins.throw "x") [ {} ])
  (builtins.foldl' (x: y: x // y) {} [ {} { a = builtins.throw "z"; } {} ])
  (builtins.foldl' (x: y: x // y) {} [ {} { b = 3; a = builtins.throw "u"; } {} ])
]
