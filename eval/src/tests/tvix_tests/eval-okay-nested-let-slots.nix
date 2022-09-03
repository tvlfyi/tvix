# This test deals with a tricky edge-case around scopes, where the
# stack slot accounting must correctly account for the position at
# which the body of a let expression is being initialised when
# resolving upvalues.

let
  a = 1;
  b = 2;
  outer = let
    c = 3;
    d = 4;
    inner = let
      e = 5;
      f = 6;
    in g: [ a b c d e f g ];
  in inner;
in outer 7
