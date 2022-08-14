# `or` operator should keep working if it encounters a non-set type.
{ a.b = 42; }.a.b.c or "works fine"
