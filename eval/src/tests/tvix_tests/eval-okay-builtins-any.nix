[
  (builtins.any (x: x) [ ])
  (builtins.any (x: x) [ true true true ])
  (builtins.any (x: x) [ false false false ])
  (builtins.any (x: x) [ true true false ])
  (builtins.any (x: x) [ false true true ])

  # evaluation should short-circuit
  (builtins.any (x: x) [ false true (builtins.abort "should be unreachable") ])

  # arbitrary functions supported
  (builtins.any (x: x * 2 == 42) [ ])
  (builtins.any (x: x * 2 == 42) [ 7 21 42 ])
  (builtins.any (x: x * 2 == 42) [ 1 2 3 ])
]
