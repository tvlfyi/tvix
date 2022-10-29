let
  keys = [ 1 2 3 2 2 2 5 0 5 5 4 3 2 1 2 3 22 ];
in

builtins.sort
  (a: b: a.key < b.key)
  (builtins.genList (index: { inherit index; key = builtins.elemAt keys index; }) (builtins.length keys))
