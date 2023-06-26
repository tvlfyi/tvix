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

  # Surprisingly the following expressions don't work. They are
  # here for documentation purposes and covered only
  # by eval-okay-select-pointer-inequality.nix. Reasoning is that
  # we may not want / be able to replicate this behavior at all.
  #   ([ builtins.add ] == [ builtins.add ])
  #   ({ inherit (builtins) import; } == { inherit (builtins) import; })

  # These expressions work as expected, however:
  (let x = { inherit (builtins) add; }; in x == x)
  (let inherit (builtins) add; in [ add ] == [ add ])
]
