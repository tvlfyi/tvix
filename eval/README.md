Tvix Evaluator
==============

This project implements an interpreter for the Nix programming
language.

The interpreter aims to be compatible with `nixpkgs`, on the
foundation of Nix 2.3.

<!-- TODO(tazjin): Remove this note when appropriate -->
Work on this project is *extremely in-progress*, and the state of the
project in the public repository may not necessarily reflect the state
of the private codebase, as we are slowly working on publishing it.

We expect this to have caught up in a handful of weeks (as of
2022-08-12).

Please contact [TVL](https://tvl.fyi) with any questions you might
have.

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
