[
  (builtins.split "(a)b" "abc")
  (builtins.split "([ac])" "abc")
  (builtins.split "(a)|(c)" "abc")
  (builtins.split "([[:upper:]]+)" " FOO ")

  (builtins.split "(.*)" "abc")
  (builtins.split "([abc]*)" "abc")
  (builtins.split ".*" "")
]
