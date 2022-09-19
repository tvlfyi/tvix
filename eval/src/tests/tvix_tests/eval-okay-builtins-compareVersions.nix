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
]
