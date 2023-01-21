Tvix Evaluator
==============

This project implements an interpreter for the Nix programming
language. You can experiment with an online version of the evaluator:
[tvixbolt][].

The interpreter aims to be compatible with `nixpkgs`, on the
foundation of Nix 2.3.

**Important note:** The evaluator is not yet feature-complete, and
while the core mechanisms (compiler, runtime, ...) have stabilised
somewhat, a lot of components are still changing rapidly.

Please contact [TVL](https://tvl.fyi) with any questions you might
have.

## Building tvix-eval

Please check the `README.md` one level up for instructions on how to build this.

The evaluator itself can also be built with standard Rust tooling (i.e. `cargo
build`).

If you would like to clone **only** the evaluator and build it
directly with Rust tooling, you can do:

```bash
git clone https://code.tvl.fyi/depot.git:/tvix/eval.git tvix-eval

cd tvix-eval && cargo build
```

## Nix test suite

C++ Nix implements a language test suite in the form of Nix source
code files, and their expected output. The coverage of this test suite
is not complete, but we intend to be compatible with it.

We have ported the test suite to Tvix, but do not run it by default as
we are not yet compatible with it.

You can run the test suite by enabling the `nix_tests` feature in
Cargo:

    cargo test --features nix_tests

## rnix-parser

Tvix is written in memory of jD91mZM2, the author of [rnix-parser][]
who sadly [passed away][rip].

Tvix makes heavy use of rnix-parser in its bytecode compiler. The
parser is now maintained by Nix community members.

[rnix-parser]: https://github.com/nix-community/rnix-parser
[rip]: https://www.redox-os.org/news/open-source-mental-health/
[tvixbolt]: https://tazj.in/blobs/tvixbolt/
