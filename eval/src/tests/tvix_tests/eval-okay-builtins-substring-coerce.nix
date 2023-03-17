# builtins.substring uses string coercion internally

builtins.substring 0 2 {
  __toString = _: "4200";
}
