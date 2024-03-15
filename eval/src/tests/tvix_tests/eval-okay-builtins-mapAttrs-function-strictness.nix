let
  self =
    let
      s = builtins.mapAttrs self.f { a = 1; b = 2; };
    in
    { f = _: x: x; } // s;
in
self
