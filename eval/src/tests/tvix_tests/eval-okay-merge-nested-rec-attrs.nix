{
  set = rec {
    a = 21;
  };

  set = {
    # Fun fact: This might be the only case in Nix where a lexical
    # resolution of an identifier can only be resolved by looking at
    # *siblings* in the AST.
    b = 2 * a;
  };
}
