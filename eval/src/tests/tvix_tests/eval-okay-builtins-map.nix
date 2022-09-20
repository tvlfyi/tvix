[
  # identity function
  (builtins.map (x: x) [ 1 2 3 4 5 ])

  # double stuff
  (builtins.map (x: x * 2) [ 1 2 3 4 5 ])

  # same but with a closure this time
  (
    let n = 2;
    in builtins.map (x: x * n) [ 1 2 3 4 5 ]
  )

  # same, but with a builtin
  (builtins.map (builtins.mul 2) [ 1 2 3 4 5 ])

  # from global scope
  (map (x: x) [ 1 2 3 4 5 ])
]
