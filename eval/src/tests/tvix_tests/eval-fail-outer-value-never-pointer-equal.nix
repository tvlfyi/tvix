# For an explanation of this behavior see //tvix/docs/value-pointer-equality.md
let
  x = { foo = throw "foo"; };
in

# while `builtins.seq x null` would succeed, this fails!
x == x
