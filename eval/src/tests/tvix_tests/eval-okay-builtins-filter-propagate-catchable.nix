map (e: (builtins.tryEval e).success) [ (builtins.filter (builtins.throw "a") [ "" ]) (builtins.filter (x: true) (builtins.throw "b")) (builtins.filter (_: builtins.throw "x") [ "" ]) ]
