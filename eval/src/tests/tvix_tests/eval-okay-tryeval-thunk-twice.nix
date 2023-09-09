let x = throw "lol"; in builtins.map (f: f x) [ builtins.tryEval builtins.tryEval ]
