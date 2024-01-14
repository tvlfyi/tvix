map (e: (builtins.tryEval e).success) [ (builtins.all (builtins.throw "a") [ "" ]) (builtins.all (x: true) (builtins.throw "b")) (builtins.all (_: builtins.throw "x") [ "" ]) ]
