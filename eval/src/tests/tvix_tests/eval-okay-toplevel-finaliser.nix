# A simple expression with upvalue resolution beyond the target stack
# index of the root expression.

let
  a = 1;
  b = 2;
  c = 3;
in {
  inherit a b c;
}
