[
  (builtins.compareVersions "1.2.3" "1.2.3")
  (builtins.compareVersions "1.2.2" "1.2.3")
  (builtins.compareVersions "1.2.3" "1.2.40")
  (builtins.compareVersions "1.2.3" ".1.2.3")
  (builtins.compareVersions "1.2.3" "1..2.3")
  (builtins.compareVersions "1.2.3" "1.2.3.")
  (builtins.compareVersions "1.2.3" "1.2")
  (builtins.compareVersions "1.2.3" "1.2.a")
  (builtins.compareVersions "1a.b" "1a.2")
  (builtins.compareVersions "1" "")
]
