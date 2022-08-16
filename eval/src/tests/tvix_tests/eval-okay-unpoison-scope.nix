let
  poisoned = let
    true = 1;
    false = 2;
    null = 3;
  in [ true false null ];
in [ true false null ] ++ poisoned
