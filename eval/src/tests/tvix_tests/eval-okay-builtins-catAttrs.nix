builtins.catAttrs "foo" [
  { foo = 21; }
  { bar = 23; foo = "+"; }
  { }
  { bar = 12; }
  { foo = 21 + 0; }
  { foo = "="; }
  ({ bar = 13; } // { baz = 89; })
  { foo = 42; bar = 33; }
]
