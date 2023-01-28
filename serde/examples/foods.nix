# This is content for the `Data` struct, written in intentionally
# convoluted Nix code.
let
  mkFlavour = flavour: name: {
    inherit name;
    value = flavour;
  };

  tasty = mkFlavour "Tasty";
  okay = mkFlavour "Okay";
  eww = mkFlavour "Eww";
in
{
  name = "exhaustive list of foods";

  foods = builtins.listToAttrs [
    (tasty "beef")
    (okay "tomatoes")
    (eww "olives")
    (tasty "coffee")
  ];
}
