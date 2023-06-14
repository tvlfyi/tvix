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

## Tests

Tvix currently has three language test suites for tvix-eval:

* `nix_tests` and `tvix_tests` are based on the same mechanism
  borrowed from the C++ Nix implementation. They consist of
  Nix files as well as expected output (if applicable).
  The test cases are split into four categories:
  `eval-okay` (evaluates successfully with the expected output),
  `eval-fail` (fails to evaluate, no expected output),
  `parse-okay` (expression parses successfully, no expected output)
  and `parse-fail` (expression fails to parse, no expected output).
  Tvix currently ignores the last two types of test cases, since
  it doesn't implement its own parser.

  Both test suites have a `notyetpassing` directory. All test cases
  in here test behavior that is not yet supported by Tvix. They are
  considered to be expected failures, so you can't forget to move
  them into the test suite proper when fixing the incompatibility.

  Additionally, separate targets in the depot pipeline, under
  `//tvix/verify-lang-tests`, check both test suites (including
  `notyetpassing` directories) against
  C++ Nix 2.3 and the default C++ Nix version in nixpkgs.
  This way we can prevent accidentally introducing test cases
  for behavior that C++ Nix doesn't exhibit.

  * `nix_tests` has the test cases from C++ Nix's language test
    suite and is sporadically updated by manually syncing the
    directories. The `notyetpassing` directory shows how far
    it is until we pass it completely.

  * `tvix_tests` contains test cases written by the Tvix contributors.
    Some more or less duplicate test cases contained in `nix_tests`,
    but many cover relevant behavior that isn't by `nix_tests`.
    Consequently, it'd be nice to eventually merge the two test
    suites into a jointly maintained, common Nix language test suite.

    It also has a `notyetpassing` directory for missing behavior
    that is discovered while working on Tvix and isn't covered by the
    `nix_tests` suite.

* `nix_oracle` can evaluate Nix expressions in Tvix and compare the
  result against C++ Nix (2.3) directly. Eventually it should gain
  the ability to property test generated Nix expressions.
  An additional feature is that it can evaluate expressions without
  `--strict`, so thunking behavior can be verified more easily.

## rnix-parser

Tvix is written in memory of jD91mZM2, the author of [rnix-parser][]
who sadly [passed away][rip].

Tvix makes heavy use of rnix-parser in its bytecode compiler. The
parser is now maintained by Nix community members.

[rnix-parser]: https://github.com/nix-community/rnix-parser
[rip]: https://www.redox-os.org/news/open-source-mental-health/
[tvixbolt]: https://tvixbolt.tvl.su/
