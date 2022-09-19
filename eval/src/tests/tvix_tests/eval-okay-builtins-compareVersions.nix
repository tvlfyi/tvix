let
  cmp = a: b:
    let
      ord1 = builtins.compareVersions a b;
      ord2 = builtins.compareVersions b a;
    in
      assert ord1 == -ord2; ord1;
in

[
  (cmp "1.2.3" "1.2.3")
  (cmp "1.2.2" "1.2.3")
  (cmp "1.2.3" "1.2.40")
  (cmp "1.2.3" ".1.2.3")
  (cmp "1.2.3" "1..2.3")
  (cmp "1.2.3" "1.2.3.")
  (cmp "1.2.3" "1.2")
  (cmp "1.2.3" "1.2.a")
  (cmp "1a.b" "1a.2")
  (cmp "1" "")
  (cmp "1.0" "1.0.0")
  # Subset of test cases from eval-okay-versions.nix shipped by C++ Nix
  (cmp "1.0" "2.3")
  (cmp "2.1" "2.3")
  (cmp "2.3" "2.3")
  (cmp "2.5" "2.3")
  (cmp "3.1" "2.3")
  (cmp "2.3.1" "2.3")
  (cmp "2.3.1" "2.3a")
  (cmp "2.3pre1" "2.3")
  (cmp "2.3pre3" "2.3pre12")
  (cmp "2.3a" "2.3c")
  (cmp "2.3pre1" "2.3c")
  (cmp "2.3pre1" "2.3q")
]
