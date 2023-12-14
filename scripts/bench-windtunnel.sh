#!/usr/bin/env nix-shell
#!nix-shell -i bash ../.. -A tvix.shell

# Benchmark script that runs inside the Windtunnel build agent

set -euo pipefail

echo "Running benchmarks for tvix/eval..."
pushd "$(dirname "$(dirname "$0")")/eval"
cargo bench
windtunnel-cli report -f criterion-rust .
popd

echo "Running tvix macrobenchmarks..."
pushd "$(dirname "$(dirname "$0")")"

depot_nixpkgs_path="$(nix eval --raw '("${((import ../third_party/sources {}).nixpkgs)}")')"
pinned_nixpkgs_path="$(nix eval --raw '(builtins.fetchTarball {url = "https://github.com/NixOS/nixpkgs/archive/91050ea1e57e50388fa87a3302ba12d188ef723a.tar.gz"; sha256 = "1hf6cgaci1n186kkkjq106ryf8mmlq9vnwgfwh625wa8hfgdn4dm";})')"

cargo build --release --bin tvix
hyperfine --export-json ./results.json \
    -n 'tvix-eval-depot-nixpkgs-hello' "target/release/tvix -E '(import ${depot_nixpkgs_path} {}).hello.outPath'" \
    -n 'tvix-eval-depot-nixpkgs-cross-hello' "target/release/tvix -E '(import ${depot_nixpkgs_path} {}).pkgsCross.aarch64-multiplatform.hello.outPath'" \
    -n 'tvix-eval-pinned-nixpkgs-hello' "target/release/tvix -E '(import ${pinned_nixpkgs_path} {}).hello.outPath'" \
    -n 'tvix-eval-pinned-nixpkgs-cross-hello' "target/release/tvix -E '(import ${pinned_nixpkgs_path} {}).pkgsCross.aarch64-multiplatform.hello.outPath'"
windtunnel-cli report -f hyperfine-json ./results.json
popd
