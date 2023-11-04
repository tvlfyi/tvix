#! /usr/bin/env nix-shell
#! nix-shell -i "bash -e"
#! nix-shell -p cargo-lambda
cargo lambda build --release
cargo lambda deploy
