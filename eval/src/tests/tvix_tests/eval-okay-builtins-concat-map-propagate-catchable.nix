map (e: (builtins.tryEval e).success) [ (builtins.concatMap (builtins.throw "a") [ "" ]) (builtins.concatMap (_: builtins.throw "x") [ "" ]) (builtins.concatMap (_: []) (builtins.throw "a")) ]
