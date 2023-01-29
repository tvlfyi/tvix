# Tests formals which have internal default values.

({ defaultValue, optional ? defaultValue }: optional) { defaultValue = 42; }
