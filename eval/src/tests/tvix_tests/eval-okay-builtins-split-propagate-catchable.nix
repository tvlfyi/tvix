map (e: (builtins.tryEval e).success) [
  (builtins.split (builtins.throw "regex") "abc")
  (builtins.split "[^/]" (builtins.throw "string"))
]
