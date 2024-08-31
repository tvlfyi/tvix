# This file is shell.nix in the tvix josh workspace,
# *and* used to provide the //tvix:shell attribute in a full depot checkout.
# Hence, it may not use depot as a toplevel argument.

{
  # This falls back to the tvix josh workspace-provided nixpkgs checkout.
  # In the case of depot, it's always set explicitly.
  pkgs ? (import ./nixpkgs {
    depotOverlays = false;
    depot.third_party.sources = import ./sources { };
    additionalOverlays = [
      (self: super: {
        # macFUSE bump containing fix for https://github.com/osxfuse/osxfuse/issues/974
        # https://github.com/NixOS/nixpkgs/pull/320197
        fuse =
          if super.stdenv.isDarwin then
            super.fuse.overrideAttrs
              (old: rec {
                version = "4.8.0";
                src = super.fetchurl {
                  url = "https://github.com/osxfuse/osxfuse/releases/download/macfuse-${version}/macfuse-${version}.dmg";
                  hash = "sha256-ucTzO2qdN4QkowMVvC3+4pjEVjbwMsB0xFk+bvQxwtQ=";
                };
              }) else super.fuse;
      })
    ];
  })
, ...
}:

pkgs.mkShell {
  name = "tvix-rust-dev-env";
  packages = [
    pkgs.buf-language-server
    pkgs.cargo
    pkgs.cargo-machete
    pkgs.cargo-expand
    pkgs.clippy
    pkgs.d2
    pkgs.evans
    pkgs.fuse
    pkgs.go
    pkgs.grpcurl
    pkgs.hyperfine
    pkgs.mdbook
    pkgs.mdbook-admonish
    pkgs.mdbook-d2
    pkgs.mdbook-plantuml
    pkgs.nix_2_3 # b/313
    pkgs.pkg-config
    pkgs.rust-analyzer
    pkgs.rustc
    pkgs.rustfmt
    pkgs.plantuml
    pkgs.protobuf
  ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
    # We need these two dependencies in the ambient environment to be able to
    # `cargo build` on MacOS.
    pkgs.libiconv
    pkgs.buildPackages.darwin.apple_sdk.frameworks.Security
  ];

  # Set TVIX_BENCH_NIX_PATH to a somewhat pinned nixpkgs path.
  # This is for invoking `cargo bench` imperatively on the developer machine.
  # For tvix benchmarking across longer periods of time (by CI), we probably
  # should also benchmark with a more static nixpkgs checkout, so nixpkgs
  # refactorings are not observed as eval perf changes.
  shellHook = ''
    export TVIX_BENCH_NIX_PATH=nixpkgs=${pkgs.path}
  '';
}
