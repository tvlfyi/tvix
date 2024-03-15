let
  id = x: x;
in

builtins.genericClosure {
  startSet = [{ key = id; first = true; }];
  operator =
    { first, ... }:
    if first then [
      { key = id; first = false; }
    ] else [ ];
}
