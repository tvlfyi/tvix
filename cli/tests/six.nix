{ }: { six = builtins.foldl' (x: y: x + y) 0 [ 1 2 3 ]; }
