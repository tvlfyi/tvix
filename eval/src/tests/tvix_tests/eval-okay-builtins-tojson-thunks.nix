let
  a = b * 2;
  b = 21;
in
builtins.toJSON [
  a
  ((n: n * 2) 21)
  (builtins.toJSON a)
]
