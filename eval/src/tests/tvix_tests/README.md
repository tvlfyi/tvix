These tests are "native" to Tvix and exist in addition to the Nix test
suite.

All of these are straightforward code snippets which are expected to
produce a certain result.

# `identity-*` tests

Files named `identity-*.nix` contain code that is supposed to produce
itself exactly after evaluation.

These are useful for testing literals.

# `eval-okay-*` tests

Files named `eval-okay-*.nix` contain code which is supposed to
evaluate to the output in the corresponding `eval-okay-*.exp` file.

This convention is taken from the original Nix test suite.
