let
  a = {
    a = 3;
    b = b.b;
  };

  b = {
    a = a.a - 2;
    b = 2;
    c = a.c or 3;
  };
in

a // b
