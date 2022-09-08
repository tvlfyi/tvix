# This test mixes different ways of creating bindings in a let … in expression
# to make sure that the compiler initialises the locals in the same order as
# they are declared.

let
  d = 4;
in

# Trick to allow useless inherits in the following let
with { _unused = null; };

let
  set = { b = 2; };
  a = 1;
  inherit (set) b;
  c = 3;
  inherit d;
in

[ a b c d ]
