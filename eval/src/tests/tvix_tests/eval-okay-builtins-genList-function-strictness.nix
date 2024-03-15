let
  self =
    let
      l = builtins.genList (builtins.head self) 3;
    in
    [ (x: x) ] ++ l;
in
self
