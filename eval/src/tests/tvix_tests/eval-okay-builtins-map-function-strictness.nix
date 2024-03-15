let
  self =
    let
      l = builtins.map (builtins.head self) [ 2 "." 18 https://github.com/NixOS/nix/issues/9779 "-.-" ];
    in
    [ (x: x) ] ++ l;
in
self
