# Note: the attribute values in this set aren't just dummies!  They
# are booleans which indicate whether or not the corresponding
# attrname is valid without quotification.
{
  __internal = true;
  _internal = true;
  normal = true;
  VeryNormal = true;
  normal2 = true;
  Very2Normal = true;
  _'12 = true;
  foldl' = true;
  x = true;
  x' = true;
  x'' = true;

  true = true;
  false = true;
  null = true;
  or = true;
  "assert" = false;
  throw = true;
  abort = true;

  "9front" = false;
  "2normal" = false;
  "-20°" = false;
  "45 44 43-'3 2 1" = false;
  "attr.path" = false;
  "'quoted'" = false;
  "_'12.5" = false;
  "😀" = false;

  "if" = false;
  "then" = false;
  "else" = false;
  "with" = false;
  "let" = false;
  "in" = false;
  "rec" = false;
  "inherit" = false;
}
