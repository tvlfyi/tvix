(builtins.tryEval (({ foo ? throw "up" }: if foo then 1 else 2) { })).success
