[
  (builtins.baseNameOf /foo/bar)
  (builtins.baseNameOf "foo")
  (builtins.baseNameOf "foo///")
  (builtins.baseNameOf "foo/bar")
  (builtins.baseNameOf "./.")
  (builtins.baseNameOf "")
  (builtins.baseNameOf /.)
  (builtins.toString (builtins.baseNameOf /.))
]
