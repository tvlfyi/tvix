# Nix allows using the ? operator on non-set types, in which case it
# should always return false.
[ (123 ? key) ("foo" ? key) (null ? key) ([ "key" ] ? key) ]
