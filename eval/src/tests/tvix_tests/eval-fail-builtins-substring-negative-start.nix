# Negative start is illegal, but negative length works, see
# eval-okay-builtins-substring-negative-length.nix
builtins.substring (-1) 1 "Wiggly Donkers"
