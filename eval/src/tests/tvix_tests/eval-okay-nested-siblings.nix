rec {
  outer =
    let inner = sibling;
    in inner;

  sibling = 42;
}
