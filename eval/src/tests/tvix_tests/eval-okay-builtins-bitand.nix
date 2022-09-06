[
  (builtins.bitAnd 0 0)
  (builtins.bitAnd 0 1)
  (builtins.bitAnd 1 0)
  (builtins.bitAnd 1 1)
  (builtins.bitAnd 8 8)
  (builtins.bitAnd 8 (builtins.add 4 4))
  (builtins.bitAnd (builtins.add 4 4) 8)
  (builtins.bitAnd (builtins.add 4 4) (builtins.add 4 4))
]
