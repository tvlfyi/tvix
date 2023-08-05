[
  (builtins.div 9 3)
  (builtins.div 7 1)
  (builtins.div 3 9)
  (builtins.div 4 4)
  (builtins.div 1 2)
  (builtins.div 1.0 2)
  (builtins.div 1 2.0)
  (builtins.div 1.0 2.0)
  (builtins.div (builtins.div 84 4) 0.5)

  # builtins.div should truncate towards 0
  (builtins.div 3 2)
  (builtins.div (-3) (-2))
  (builtins.div (-3) 2)
  (builtins.div 3 (-2))
  (-(builtins.div 3 2))

  (builtins.div 4 3)
  (builtins.div (-4) (-3))
  (builtins.div (-4) 3)
  (builtins.div 4 (-3))
  (-(builtins.div 4 3))

  (builtins.div 5 3)
  (builtins.div (-5) (-3))
  (builtins.div (-5) 3)
  (builtins.div 5 (-3))
  (-(builtins.div 5 3))

  (builtins.div 2147812578 (-28748))
  (builtins.div (-2147812578) 28748)
  (-(builtins.div 2147812578 28748))
]
