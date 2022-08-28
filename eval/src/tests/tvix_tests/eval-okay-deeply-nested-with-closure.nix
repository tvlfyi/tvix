# This convoluted test constructs a situation in which dynamically
# resolved upvalues refer `with` blocks introduced at different lambda
# context boundaries, i.e. the access to a, b in the innermost closure
# must be threaded through upvalues in several levels.

(_:
with { a = 1; b = 1; };

_:
with { b = 2; c = 2; };

_:
with { c = 3; d = 3; };

_:
with { d = 4; };

[ a b c d ]) null null null null
