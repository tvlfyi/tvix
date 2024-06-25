let
  noContext = (builtins.toFile "foo" "bar");
  someContext = (builtins.toFile "foo" "bar${noContext}");
  moreContext = (builtins.toFile "foo" "bar${someContext}");
in
[
  noContext
  someContext
  moreContext
  (builtins.getContext moreContext)
]
