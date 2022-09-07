let
  inherit (c) d;
  inherit (a) b c;

  a = {
    b = 20;
    c = {
      d = 3;
    };
  };
in

b + d
