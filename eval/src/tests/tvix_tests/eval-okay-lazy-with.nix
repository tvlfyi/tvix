# The 'namespace' of a with should only be evaluated if an identifier
# from it is actually accessed.

with (builtins.throw "should not occur");

42
