map (e: (builtins.tryEval e).success) [ (builtins.catAttrs "a" (builtins.throw "b")) (builtins.catAttrs (builtins.throw "a") { a = 1; }) ]
