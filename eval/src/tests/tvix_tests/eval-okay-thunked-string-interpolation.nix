let
  final = { text = "strict literal"; inherit x y; };
  x = "lazy ${throw "interpolation"}";
  y = "${throw "also lazy!"}";
in

final.text
