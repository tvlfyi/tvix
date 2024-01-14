map (e: (builtins.tryEval e).success) [
  # This one may be hard to read for non-experts.
  # Replace strings is a special built-in compared to others in the sense
  # it might attempt to lazily evaluate things upon successful replacements,
  # so it would not be surprising that some of the non-replacements which could throw
  # could be ignored by laziness. It is not the case though.
  (builtins.replaceStrings [ "a" (builtins.throw "b") ] [ "c" "d" ] "ab")
  (builtins.replaceStrings [ "a" (builtins.throw "b") ] [ "c" "d" ] "a")
  (builtins.replaceStrings [ "a" "b" ] [ "c" (builtins.throw "d") ] "a")
  (builtins.replaceStrings [ "a" "b" ] [ "c" (builtins.throw "d") ] "ab")
  (builtins.replaceStrings [ "" ] [ (builtins.throw "d") ] "ab")
  (builtins.replaceStrings [ "a" "" ] [ "b" (builtins.throw "d") ] "ab")
  (builtins.replaceStrings (builtins.throw "z") [ ] "ab")
  (builtins.replaceStrings [ ] (builtins.throw "z") "ab")
  (builtins.replaceStrings [ ] [ ] (builtins.throw "z"))
]
