(builtins.tryEval { "${builtins.throw "a"}" = "b"; }).success
