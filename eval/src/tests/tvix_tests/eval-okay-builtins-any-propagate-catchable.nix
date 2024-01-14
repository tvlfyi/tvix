map (e: (builtins.tryEval e).success) [ (builtins.any (builtins.throw "a") [ "" ]) (builtins.any (x: true) (builtins.throw "b")) (builtins.any (_: builtins.throw "a") [ "" ]) ]
