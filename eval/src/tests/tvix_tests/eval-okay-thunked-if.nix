let
  a = { };
in
let
  c = if builtins.isFunction a then a b else a;
  b = { };
in
c
