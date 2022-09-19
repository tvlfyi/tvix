let
  # apply is thunked, so we can create a thunked value using the identity function
  thunk = x: x;
in
[
  (builtins.isAttrs { bar = throw "baz"; })
  (builtins.isAttrs (thunk { foo = 13; }))
  (builtins.isAttrs (thunk 123))
  (builtins.isBool true)
  (builtins.isBool (thunk false))
  (builtins.isBool (thunk "lol"))
  (builtins.isFloat 1.2)
  (builtins.isFloat (thunk (1 * 1.0)))
  (builtins.isFloat 1)
  (builtins.isFunction thunk)
  (builtins.isFunction (thunk thunk))
  (builtins.isFunction {})
  (builtins.isInt 1)
  (builtins.isInt (thunk 42))
  (builtins.isInt 1.0)
  (builtins.isList [ (throw "oh no") (abort "it's over") ])
  (builtins.isList (thunk [ 21 21 ]))
  (builtins.isList (thunk {}))
  (builtins.isNull null)
  (builtins.isNull (thunk null))
  (builtins.isNull 42)
  (builtins.isPath ./relative)
  (builtins.isPath (thunk /absolute))
  (builtins.isPath "/not/a/path")
  (builtins.isString "simple")
  (builtins.isString "${{ outPath = "coerced"; }}")
  (builtins.isString "hello ${"interpolation"}")
  (builtins.isString true)
]
