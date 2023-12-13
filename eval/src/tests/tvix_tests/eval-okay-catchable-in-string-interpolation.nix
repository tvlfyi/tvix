(builtins.tryEval ("${toString 3}  ${throw "bob"}")).success
