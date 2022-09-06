# Tests correct tracking of stack indices within construction of an
# attribute set. Dynamic keys can be any expression, so something that
# is extremely sensitive to stack offsets (like `with`) can be tricky.

let
  set1 = { key = "b"; };
  set2 = {
    a = 20;
    ${with set1; key} = 20;
    ${with { key = "c"; }; key} = 2;
  };
in set2.a + set2.b + set2.c
