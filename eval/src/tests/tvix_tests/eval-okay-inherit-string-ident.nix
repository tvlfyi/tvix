# identifiers in inherits can be string-like expressions

let
  set = {
    inherit ({ value = 42; }) "value";
  };
in set.value
