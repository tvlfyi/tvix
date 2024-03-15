let
  set = {
    a = 1;
  };
in
let
  set2 = {
    b = 1;
  };
  inherit (set) a;
  inherit (set2) b;
in
a + b
