# Manual desugaring of something similar to `rec`, to test lower level
# recursion primitives.

let
  set = with set; {
    a = 21;
    b = a * 2;
  };
in
set.b
