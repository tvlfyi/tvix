let
  makeTrue = _: true;
  makeFalse = _: false;
in
[
  # useless `false`
  (false || makeTrue null) # true
  (makeTrue null || false) # true

  # useless `true`
  (true && makeFalse null) # false
  (makeFalse null && true) # false

  # useless `||`
  (true || makeFalse null) # true
  (makeFalse null || true) # true

  # useless `&&`
  (false && makeTrue null) # false
  (makeTrue null && false) # false
]
