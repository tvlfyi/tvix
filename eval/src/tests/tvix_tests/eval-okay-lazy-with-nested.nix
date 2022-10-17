# The 'namespace' of a with should only be evaluated if an identifier
# from it is actually accessed.

with (abort "should not be evaluated");
let a = dynamic; in 42
