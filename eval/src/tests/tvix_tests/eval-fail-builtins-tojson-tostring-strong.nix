# String coercions when using builtins.toJSON on an attribute set with
# a `__toString` attribute should be weak.
builtins.toJSON {
  __toString = self: self.x;
  x = 42;
}
