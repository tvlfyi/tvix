let
  # self-recursive function should be able to close over itself
  f = n: if n <= 0 then "done" else f (n - 1);
in
f 10
