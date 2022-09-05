[
  (builtins.attrNames {})
  (builtins.attrNames { foo = 1; bar = 2; baz = 3; })
  (builtins.attrNames { Foo = 1; bar = 2; Baz = 3; })
  (builtins.attrNames {
    "Graham Chapman" = true;
    "John Cleese" = true;
    "Terry Gilliam" = true;
    "Eric Idle" = true;
    "Terry Jones" = true;
    "Michael Palin" = true;
  })
]
