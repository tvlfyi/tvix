assert true;

let
  x = assert false; 13;
  y = 12;
in

{ inherit x y; }.y
