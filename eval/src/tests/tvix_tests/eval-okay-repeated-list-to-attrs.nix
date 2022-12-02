# Ensure that builtins.listToAttrs returns the first instance of a key.

let
  inherit (builtins) foldl' listToAttrs;

  input = [ { name = "result"; value = 1; } { name = "result"; value = 2; } ];

  # foldl-based version of listToAttrs with the _opposite_ behaviour.
  listToAttrs' = list: foldl' ( acc: elem: acc // { ${elem.name} = elem.value; }) {} list;
in [
  (listToAttrs input).result
  (listToAttrs' input).result
]
