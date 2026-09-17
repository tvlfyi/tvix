# This file is shell.nix in the tvix josh workspace,
# *and* used to provide the //tvix:shell attribute in a full depot checkout.
# Hence, it may not use depot as a toplevel argument.

{
  # This falls back to the tvix josh workspace-provided nixpkgs checkout.
  # In the case of depot, it's always set explicitly.
  pkgs ? (import ./nixpkgs {
    depotOverlays = false;
    depot.third_party.sources = import ./sources { };
    # otherwise nix_2_3 is a throwing alias
    externalArgs.nixpkgsConfig.allowAliases = false;
  })
, ...
}:

let
  # latest C++ Nix fails nix_oracle, see b/313
  nix = pkgs.nix_2_3 or pkgs.lix;

  # Lix deprecated url-literals which we still want to test
  lix-instantiate-wrapper = pkgs.runCommand "lix-instantiate-wrapper"
    {
      nativeBuildInputs = [ pkgs.buildPackages.makeWrapper ];
    } ''
    makeWrapper "${nix}/bin/nix-instantiate" "$out" \
      --add-flags "--extra-deprecated-features url-literals"
  '';
in

pkgs.mkShell {
  name = "tvix-rust-dev-env";
  packages = [
    pkgs.cargo
    pkgs.cargo-machete
    pkgs.cargo-expand
    pkgs.clippy
    pkgs.hyperfine
    nix
    pkgs.pkg-config
    pkgs.rustc
    pkgs.rustfmt
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
    export TVIX_BUILD_SANDBOX_SHELL=${if pkgs.stdenv.hostPlatform.isLinux then pkgs.busybox-sandbox-shell + "/bin/busybox" else "/bin/sh"}
    export TVIX_BENCH_NIX_PATH=nixpkgs=${pkgs.path}
  '' + pkgs.lib.optionalString (nix.pname == "lix") ''
    export NIX_INSTANTIATE_BINARY_PATH="${lix-instantiate-wrapper}"
  '';
}
