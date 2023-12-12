(builtins.tryEval (({ foo ? throw "up" }: foo -> true) { })).success
