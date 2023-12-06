#!/usr/bin/env bash

# Benchmark script that runs inside the Windtunnel build agent

set -euo pipefail

echo "Running benchmarks for tvix/eval..."
cd "$(dirname "$(dirname "$0")")/eval"
docker run --rm -v "$(pwd):/app" -w /app rust cargo bench
windtunnel-cli report -f criterion-rust .
