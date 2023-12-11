#!/usr/bin/env nix-shell
#!nix-shell -i bash ../.. -A tvix.shell

# Benchmark script that runs inside the Windtunnel build agent

set -euo pipefail

echo "Running benchmarks for tvix/eval..."
cd "$(dirname "$(dirname "$0")")/eval"
cargo bench
windtunnel-cli report -f criterion-rust .

echo "Running tvix macrobenchmarks..."
cargo build --release --bin tvix
hyperfine --export-json ./results.json \
    -n 'eval-nixpkgs-hello' "target/release/tvix -E '(import ../../nixpkgs {}).hello.outPath'" \
    -n 'eval-nixpkgs-cross-hello' "target/release/tvix -E '(import ../../nixpkgs {}).pkgsCross.aarch64-multiplatform.hello.outPath'"
windtunnel-cli report -f hyperfine-json ./results.json
