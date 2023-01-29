# Tests formals which have internal default values that must be deferred.

({ optional ? defaultValue, defaultValue }: optional) { defaultValue = 42; }
