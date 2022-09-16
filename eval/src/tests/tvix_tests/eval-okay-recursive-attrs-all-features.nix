let a = 1;
in
(rec {
  inherit a;

  b = {
    c = a + 20;
  };

  inherit (b) c;

  d = c * 2;
}).d
