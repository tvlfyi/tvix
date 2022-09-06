# Creates a `with` across multiple thunk boundaries.

let
  set = {
    a = with { b = 42; }; b;
  };
in set.a
