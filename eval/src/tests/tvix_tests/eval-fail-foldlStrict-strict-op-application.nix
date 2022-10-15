builtins.foldl'
  (_: f: f null)
  (throw "This doesn't explode")
  [ (_: throw "Not the final value, but is still forced!") (_: 23) ]
