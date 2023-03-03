# Attribute sets with an `outPath` can contain _any_ serialisable
# value in that field.
builtins.toJSON {
  outPath = {
    a = 40;
    b = 2;
  };
}
