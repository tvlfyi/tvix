# Normally using an `inherit` without a source attribute set within a
# `let` is a no-op, *unless* there is a with in-scope that might
# provide the value.

# Provide a dynamic `x` identifier in the scope.
with ({ x = 1; });

# inherit this `x` as a static identifier
let inherit x;

  # Provide another dynamic `x` identifier
in
with ({ x = 3; });

# Inherited static identifier should have precedence
x
