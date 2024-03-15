let
  condition = x: y: x < y;
in

# The function application here will become a thunk which verifies that
  # assert forces the condition expression correctly.
assert condition 21 42; 21
