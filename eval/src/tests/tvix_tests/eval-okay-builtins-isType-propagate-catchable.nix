let
  isTypeFns = [
    builtins.isAttrs
    builtins.isBool
    builtins.isFloat
    builtins.isFunction
    builtins.isInt
    builtins.isList
    builtins.isNull
    builtins.isPath
    builtins.isString
  ];
in
map (fn: (builtins.tryEval (fn (builtins.throw "is type"))).success) isTypeFns
