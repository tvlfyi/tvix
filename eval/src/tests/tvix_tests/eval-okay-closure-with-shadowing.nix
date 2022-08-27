# If a closure closes over a variable that is statically known *and*
# available dynamically through `with`, the statically known one must
# have precedence.

let
  # introduce statically known `a` (this should be the result)
  a = 1;
in

# introduce some closure depth to force both kinds of upvalue
# resolution, and introduce a dynamically known `a` within the
# closures
let f = b: with { a = 2; }; c: a + b + c;
in f 0 0
