let
  alias = builtins;
in

[
  (builtins == builtins)
  (alias == builtins)
  (builtins == builtins.builtins)
  (builtins.builtins == builtins.builtins)
  (builtins.builtins == builtins.builtins.builtins)
  (alias == alias)
  (alias == builtins.builtins)
  ([ builtins ] == [ builtins ])

  # Surprisingly this only works with the set
  ([ builtins.add ] == [ builtins.add ])
  ({ inherit (builtins) import; } == { inherit (builtins) import; })
  # But this does
  (let x = { inherit (builtins) add; }; in x == x)
]
