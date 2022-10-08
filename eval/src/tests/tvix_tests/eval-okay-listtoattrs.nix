with builtins;
let
  fold = op: nul: list:
    if list == []
    then nul
    else op (head list) (fold op nul (tail list));
  concat =
    fold (x: y: x + y) "";
  asi = name: value : { inherit name value; };
  list = [ ( asi "a" "A" ) ( asi "b" "B" ) ];
  a = builtins.listToAttrs list;
  b = builtins.listToAttrs ( list ++ list );
  r = builtins.listToAttrs [ (asi "result" [ a b ]) ( asi "throw" (throw "this should not be thrown")) ];
  x = builtins.listToAttrs [ (asi "foo" "bar") (asi "foo" "bla") ];
in concat (map (x: x.a) r.result) + x.foo
