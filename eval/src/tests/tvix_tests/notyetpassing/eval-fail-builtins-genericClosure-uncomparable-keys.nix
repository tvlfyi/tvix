# Attribute sets can't be compared, only checked for equality
builtins.genericClosure {
  startSet = [
    { key = { foo = 21; }; }
  ];
  operator = _: [
    { key = { bar = 21; }; }
  ];
}
