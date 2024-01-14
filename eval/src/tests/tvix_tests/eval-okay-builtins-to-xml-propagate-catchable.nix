map (e: (builtins.tryEval (builtins.toXML e)).success) [
  (builtins.throw "a")
  [ (builtins.throw "a") ]
  [ "abc" (builtins.throw "a") ]
  "abc${builtins.throw "c"}"
  (_: builtins.throw "d")
  {
    u = builtins.throw "x";
    v = "a";
  }
  {
    u.i.w.x.z = builtins.throw "n";
  }
  # FIXME: test derivations and files.
]
