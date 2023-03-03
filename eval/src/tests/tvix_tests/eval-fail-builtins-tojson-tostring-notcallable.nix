# attribute sets with a non-callable `__toString` can not be
# serialised to JSON.
builtins.toJSON {
  __toString = 42;
}
