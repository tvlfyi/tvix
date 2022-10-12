[
  (builtins.dirOf /foo/bar)
  (builtins.dirOf "foo")
  (builtins.dirOf "foo///")
  (builtins.dirOf "foo/bar")
  (builtins.dirOf "./.")
  (builtins.dirOf "")
  (builtins.dirOf /.)
  (builtins.toString (builtins.dirOf /.))
]
