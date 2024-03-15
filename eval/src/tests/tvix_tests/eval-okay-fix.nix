let
  fix = f: let x = f x; in x;
in
fix (self: {
  a = 1;
  b = self.a + 20;
  c = self.b * 2;
})
