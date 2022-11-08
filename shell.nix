{ depot ? import ../. { }
, pkgs ? depot.third_party.nixpkgs
, ...
}:

pkgs.mkShell {
  name = "tvix-eval-dev-env";
  packages = [
    pkgs.cargo
    pkgs.clippy
    pkgs.rust-analyzer
    pkgs.rustc
    pkgs.rustfmt
  ];
}
