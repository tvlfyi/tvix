[
  (builtins.foldl' builtins.add 0 [ 1 2 3 ])
  (builtins.foldl' (l1: l2: l1 ++ l2) [ 0 ] [ [ 1 ] [ 2 3 ] ])
  (builtins.foldl' (x: y: if x == 0 then y else x * y) 0 [ 1 2 ])
]
