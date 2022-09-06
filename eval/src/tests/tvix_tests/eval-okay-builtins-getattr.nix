[
  (builtins.getAttr "foo" { foo = 1; bar = 2; baz = 3; })
  (builtins.getAttr "bar" { foo = 1; bar = 2; baz = 3; })
  (builtins.getAttr "baz" { foo = 1; bar = 2; baz = 3; })
  (builtins.getAttr "foo" { foo = { bar = { baz = 3; }; }; })
]
