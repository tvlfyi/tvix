[
  (builtins.filter (_: true) [ 1 2 3 4 5 ])
  (builtins.filter (_: false) [ 1 2 3 4 5 ])
  (builtins.filter (x: x == 2) [ 1 2 1 2 1 2 ])

  (builtins.filter (x: (builtins.length x) > 0) [
    [ ]
    [ 1 2 ]
    [ ]
    [ ]
    [ 3 4 ]
  ])
]
