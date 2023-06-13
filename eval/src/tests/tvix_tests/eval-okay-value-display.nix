# Sanity check of how values are rendered by tvix vs. nix-instantiate(1).
# Ensures that we can use this test suite to compare against C++ Nix.
[
  null
  true
  false
  42
  42.0
  "foo\t\nbar"
  /home/arthur
  [ 1 2 3 ]
  (x: x)
  builtins.add
  (builtins.substring 0 1)
  { hello = "world"; }
]
