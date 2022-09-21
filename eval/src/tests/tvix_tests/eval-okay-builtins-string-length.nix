[
  (builtins.stringLength "foo")
  (let s = "hello"; in (builtins.substring 0 (builtins.stringLength s) s))
  (builtins.stringLength ("foo" + "${"bar" + "baz"}"))

  # feel free to delete this test case at any time, it's just to show: This is a
  # thing at the moment. We may want to break compatibility with this aspect of
  # the C++ Nix implementation at any time.
  (builtins.stringLength "😀")
]
