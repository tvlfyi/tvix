# Check some corner cases regarding escaping.
builtins.toXML { a = "s"; "&-{" = ";&\""; }
