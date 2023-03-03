# Attribute sets with an `outPath` have that outPath itself serialised
# to string.
builtins.toJSON {
  outPath = "/nix/store/jzka5ndnygkkfjfvpqwjipqp75lhz138-emacs-28.2";
}
