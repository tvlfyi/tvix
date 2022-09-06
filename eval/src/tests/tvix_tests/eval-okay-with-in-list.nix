# This code causes a situation where a list element causes an
# additional phantom value to temporarily be placed on the locals
# stack, which must be correctly accounted for by the compiler.

let
  set = {
    value = 2;
  };
in [
  1
  (with set; value)
  3
]
