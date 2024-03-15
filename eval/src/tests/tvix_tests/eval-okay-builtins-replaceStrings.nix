[
  (builtins.replaceStrings [ "oo" "a" ] [ "a" "i" ] "foobar")
  (builtins.replaceStrings [ "o" ] [ "a" ] "a")
  (builtins.replaceStrings [ "" "" ] [ "1" "2" ] "a")
  (builtins.replaceStrings [ "a" "b" "c" ] [ "A" "B" "C" ] "abc")
]
