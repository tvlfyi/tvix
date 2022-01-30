{ pkgs, lib, ... }:

let

  tl = pkgs.texlive.combine {
    inherit (pkgs.texlive) scheme-medium wrapfig ulem capt-of
      titlesec preprint enumitem paralist ctex environ svg
      beamer trimspaces zhnumber changepage framed pdfpages
      fvextra minted upquote ifplatform xstring;
  };

  csl = pkgs.fetchurl {
    name = "numeric.csl";
    url = "https://gist.githubusercontent.com/bwiernik/8c6f39cf51ceb3a03107/raw/1d75c2d62113ffbba6ed03a47ad99bde86934f2b/APA%2520Numeric";
    sha256 = "1yfhhnhbzvhrv93baz98frmgsx5y442nzhb0l956l4j35fb0cc3h";
  };

in
pkgs.stdenv.mkDerivation {
  pname = "tvix-doc";
  version = "0.1";

  outputs = [ "out" "svg" ];

  src = lib.cleanSource ./.;

  CSL = csl;

  nativeBuildInputs = [
    pkgs.pandoc
    pkgs.plantuml
    tl
  ];

  installPhase = ''
    runHook preInstall

    mkdir -p $out
    cp -v *.html $out/

    mkdir -p $svg
    cp -v *.svg $svg/

    runHook postSubmit
  '';

}
