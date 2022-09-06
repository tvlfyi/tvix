# Tests using `with` on a set that does not yet exist on the stack.

let
  result = with set; value;
  set = {
    value = 42;
  };
in result
