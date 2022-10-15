builtins.foldl'
  (_: x: x)
  (throw "This is never forced")
  [ "but the results of applying op are" 42 ]
