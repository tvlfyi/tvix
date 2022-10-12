with builtins;

let

  matches = pat: s: match pat s != null;

  splitFN = match "((.*)/)?([^/]*)\\.(nix|cc)";

in

[
  (matches "foobar" "foobar")
  (matches "fo*" "f")
  (matches "fo+" "f")
  (matches "fo*" "fo")
  (matches "fo*" "foo")
  (matches "fo+" "foo")
  (matches "fo{1,2}" "foo")
  (matches "fo{1,2}" "fooo")
  (matches "fo*" "foobar")
  (matches "[[:space:]]+([^[:space:]]+)[[:space:]]+" "  foo   ")
  (matches "[[:space:]]+([[:upper:]]+)[[:space:]]+" "  foo   ")

  (match "(.*)\\.nix" "foobar.nix")
  (match "[[:space:]]+([[:upper:]]+)[[:space:]]+" "  FOO   ")

  (splitFN "/path/to/foobar.nix")
  (splitFN "foobar.cc")
]
