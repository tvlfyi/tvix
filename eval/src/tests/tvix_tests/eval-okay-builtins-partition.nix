[
  (builtins.partition (_: true) [ 1 2 3 4 5 ])
  (builtins.partition (_: false) [ 1 2 3 4 5 ])
  (builtins.partition (x: x == 2) [ 1 2 3 4 5 ])

  (builtins.partition (x: (builtins.length x) > 1) [
    [ 1 ]
    [ 1 2 ]
    [ 2 ]
    [ 3 ]
    [ 3 4 ]
  ])
]
