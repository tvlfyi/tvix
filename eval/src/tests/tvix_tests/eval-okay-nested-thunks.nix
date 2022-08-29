# If a thunk yields another thunk, OpForce should keep forcing until
# there is a value.
let
  a = b;
  b = c;
  c = 42;
in a
