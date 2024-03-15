map (e: (builtins.tryEval e).success) [ (builtins.concatLists (builtins.throw "a")) (builtins.concatLists [ [ ] (builtins.throw "a") ]) (builtins.concatLists [ [ ] [ ] [ builtins.throw "a" ] ]) ]
