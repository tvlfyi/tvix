# tests serialisation of literal data
builtins.toJSON [
  42
  "hello"
  13.37
  [ ]
  [ 1 2 3 ]
  { }
  { name = "foo"; value = 42; }
  { foo = 42; }
]
