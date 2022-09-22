let
  # There is no syntax for accessing this identifier in an ordinary
  # way.
  "foo bar" = 42;
in ({
  # but we *can* inherit it back out
  inherit "foo bar";
})."foo bar"
