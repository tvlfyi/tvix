let
  fix = f: let x = f x; in x;
in

fix (self:
[
  (builtins.typeOf null)
  (builtins.typeOf true)
  (builtins.typeOf (true && false))
  (builtins.typeOf 12)
  (builtins.typeOf (builtins.add 21 21))
  (builtins.typeOf 1.2)
  (builtins.typeOf "foo")
  (builtins.typeOf "${"foo" + "bar"}baz")
  (builtins.typeOf { })
  # (builtins.typeOf { foo.bar = 32; }.foo) # TODO: re-enable when nested keys are done
  (builtins.typeOf ({ name = "foo"; value = 13; } // { name = "bar"; }))
  (builtins.typeOf self)
  (builtins.typeOf fix)
  (builtins.typeOf /nix/store)
]
)
