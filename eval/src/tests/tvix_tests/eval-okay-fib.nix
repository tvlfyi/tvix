let
  fib' = i: n: m: if i == 0
    then n
    else fib' (i - 1) m (n + m);

  fib = n: fib' n 1 1;
in fib 10
