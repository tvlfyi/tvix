let
  doubler = n: outer n;
  outer = let inner = n: a * n;
            a = 2;
          in inner;
in doubler 10
