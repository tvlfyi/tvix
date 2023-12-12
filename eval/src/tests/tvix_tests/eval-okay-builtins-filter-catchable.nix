(builtins.tryEval (builtins.filter (_: throw "fred") [ 3 ])).success
