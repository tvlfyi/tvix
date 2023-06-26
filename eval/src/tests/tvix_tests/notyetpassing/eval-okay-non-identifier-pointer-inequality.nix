# C++ Nix frequently creates copies of Value structs when evaluating
# a variety of expressions. As a result, pointer equality doesn't
# work for many (all?) expressions that go beyond simple identifier
# access from the scope: Even if the inner representation of the
# value still has the same memory location, C++ Nix has created
# a copy of the struct that holds the pointer to this memory.
# Since pointer equality is established via the location of
# the latter, not the former, the values are no longer equal
# by pointer.
let
  foo = { bar = x: x; };

  id = x: x;
in

[
  ({ inherit (foo) bar; } == { inherit (foo) bar; })
  ([ foo.bar ] == [ foo.bar ])

  ([ builtins.add ] == [ builtins.add ])
  ({ inherit (builtins) import; } == { inherit (builtins) import; })

  ([ (id id) ] == [ (id id) ])
  ([ id ] == [ id ])

  (with foo; [ bar ] == [ bar ])
  (with builtins; [ add ] == [ add ])
]
