builtins.tryEval (builtins.genericClosure { operator = (_: [{ key = throw "lol"; }]); startSet = [{ key = "lol"; }]; })
