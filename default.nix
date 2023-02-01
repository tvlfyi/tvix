# Externally importable TVL depot stack. This is intended to be called
# with a supplied package set, otherwise the package set currently in
# use by the TVL depot will be used.
#
{ pkgs ? (import ./nixpkgs {
    depotOverlays = false;
    depot.third_party.sources = import ./sources { };
  })
, ...
}:

let
  # `Call $methodName --bytes-as-base64` support for evans
  evans = pkgs.evans.overrideAttrs (old: {
    patches = old.patches or [ ] ++ [
      (pkgs.fetchpatch {
        url = "https://github.com/ktr0731/evans/pull/611/commits/f2109627c0d20588980fe6fd6348d223dbdf7c33.patch";
        hash = "sha256-ff8drvAYwQvHeymaHEruvwDYynClpzPM5lrB7IeQHBs=";
      })
    ];
  });
in
pkgs.mkShell {
  name = "tvix-rust-dev-env";
  packages = [
    pkgs.buf-language-server
    pkgs.cargo
    pkgs.clippy
    pkgs.rust-analyzer
    pkgs.rustc
    pkgs.rustfmt
    pkgs.protobuf

    evans
  ];
}
