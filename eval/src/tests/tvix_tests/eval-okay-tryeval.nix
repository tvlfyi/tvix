{
  w = builtins.tryEval <nope>;
  x = builtins.tryEval "x";
  y = builtins.tryEval (assert false; "y");
  z = builtins.tryEval (throw "bla");
}
