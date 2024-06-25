# in 'toFile': the file 'foo' cannot refer to derivation outputs, at (string):1:1
builtins.toFile "foo" "${(builtins.derivation {name = "foo"; builder = ":"; system = ":";})}"

