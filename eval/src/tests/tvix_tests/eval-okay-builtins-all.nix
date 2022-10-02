[
  (builtins.all (x: x) [ ])
  (builtins.all (x: x) [ true true true ])
  (builtins.all (x: x) [ false false false ])
  (builtins.all (x: x) [ true true false ])
  (builtins.all (x: x) [ false true true ])

  # evaluation should short-circuit
  (builtins.all (x: x) [ true false (builtins.abort "should be unreachable") ])

  # arbitrary functions supported
  (builtins.all (x: x * 2 == 42) [ ])
  (builtins.all (x: x * 2 == 42) [ 21 21 21 ])
  (builtins.all (x: x * 2 == 42) [ 1 2 3 ])
]
