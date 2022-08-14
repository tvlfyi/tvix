let
  set = {
    a = 1;
  };
in
  let
    inherit (set) a;
  in a
