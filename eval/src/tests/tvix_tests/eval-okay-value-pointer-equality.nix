# For an explanation of this behavior see //tvix/docs/value-pointer-equality.md
let
  # Some incomparable values
  f = MC: "Boing";
  t = [ (throw "is a little blue man") ];
  a = { "with" = abort "headphones and a big smile."; };

  # Aliases
  f' = f;
  t' = t;
  a' = a;

  peq1 = a: b: [ a ] == [ b ];
  peq2 = a: b: { x = a; } == { x = b; };
in

[
  # pointer equality of functions
  (peq1 f f)
  (peq2 f f)
  (peq1 f f')
  (peq2 f f')

  # encapsulation is necessary for pointer equality
  (f == f)
  (f == f')
  # works with !=
  ([ f ] != [ f' ])

  # thunks that fail to evaluated wrapped in sets/lists
  (peq1 t t)
  (peq2 t t)
  (peq1 a a)
  (peq2 a a)
  (peq1 t t')
  (peq2 t t')
  (peq1 a' a)
  (peq2 a' a)

  # function equality with builtins.elem
  (builtins.elem f [ 21 f 42 ])

  # pointer inequality
  (peq1 f (x: x))
  (peq2 (x: x) f)
]