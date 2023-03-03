# Attribute sets with a `__toString` attribute JSON-serialise with a
# string coercion of the function call result.

builtins.toJSON {
  __toString = self: "it's " + (builtins.toString (self.x * self.y));
  x = 21;
  y = 2;
}
