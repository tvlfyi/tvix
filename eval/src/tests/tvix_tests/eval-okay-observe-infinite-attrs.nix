# The below attribute set is infinitely large, but we should be able
# to observe it as long as we don't access its entire value.

let as = { x = 123; y = as; }; in builtins.attrNames as.y.y
