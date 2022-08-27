# Upvalues from `with` require special runtime handling. Do they work?
let
  f = with { a = 15; }; n: n * a;
in
f 10
