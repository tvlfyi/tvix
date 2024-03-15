let
  foo = x: x;
in

# key needs to be a list since it uses comparison, not equality checks:
  # lists are comparable in Nix if all non-comparable items in them are equal (e.g.
  # functions, attribute sets).
builtins.genericClosure {
  startSet = [
    { key = [{ inherit foo; }]; val = null; }
  ];
  operator = { val, ... }: if val != null then [ ] else [
    { key = [{ inherit foo; }]; val = throw "no pointer equality? 🥺👉👈"; }
  ];
}
