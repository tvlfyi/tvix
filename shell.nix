{ depot ? import ../. { }
, pkgs ? depot.third_party.nixpkgs
, ...
}:

pkgs.mkShell {
  name = "tvix-eval-dev-env";
  packages = [
    pkgs.buf-language-server
    pkgs.cargo
    pkgs.clippy
    pkgs.rust-analyzer
    pkgs.rustc
    pkgs.rustfmt
    pkgs.protobuf
  ];
}
