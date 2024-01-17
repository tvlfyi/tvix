# The Implementation Independent Nix Language Test Suite

## Design Notes

### Requirements

- It should work with potentially any Nix implementation and with all serious
  currently available ones (C++ Nix, hnix, Tvix, …). How much of it the
  implementations pass, is of course an orthogonal question.
- It should be easy to add test cases, independent of any specific
  implementation.
- It should be simple to ignore test cases and mark know failures
  (similar to the notyetpassing mechanism in the Tvix test suite).

### Test Case Types

This is a summary of relevant kinds of test cases that can be found in the wild,
usually testing some kind of concrete implementation, but also doubling up as a
potential test case for _any_ Nix implementation. For the most part, this is the
`lang` test suite of C++ Nix which is also used by Tvix and hnix.

- **parse** test cases: Parsing the given expression should either *succeed* or
  *fail*.

  - C++ Nix doesn't have any expected output for the success cases while
    `rnix-parser` checks them against its own textual AST representation.
  - For the failure cases, `rnix-parser` and C++ Nix (as of recently) have
    expected error messages/representations.

  Both error and failure cases probably are hard to implement against expected
  output/error messages for a generic test suite. Even if standardized error
  codes are implemented (see below), it is doubtful whether it'd be useful
  to have a dedicated code for every kind of parse/lex failure.
- (strict) **eval** test cases: Evaluating the given expression should either
  *fail* or *succeed* and yield a given result.

  - **eval-okay** (success) tests currently require three things:

    1. Successful evaluation after deeply forcing and printing the evaluation
       result (i.e. `nix-instantiate --eval --strict`)
    2. That the output matches an expected output exactly (string equality).
       For this the output of `nix-instantiate(1)` is used, sometimes with
       the addition of the `--xml --no-location` or `--json` flags.
    3. Optionally, stderr may need to be equal to an expected string exactly
       which would test e.g. `builtins.trace` messages or deprecation warnings
       (C++ Nix).

       This extra check is currently not supported by the Tvix test suite.

  - **eval-fail** tests require that the given expression fails to evaluate. C++
    Nix has recently started to also check the error messages via the stderr
    mechanism described above. This is not supported by Tvix at the moment.
- _lazy_ eval test cases: This is currently only supported by the `nix_oracle`
  test suite in Tvix which compares the evaluation result of expressions to the
  output of `nix-instantiate(1)` without `--strict`. By relying on the fact
  that the resulting value is not forced deeply before printing, it can be
  observed whether certain expressions are thunked or not.

  This is somewhat fragile as permissible optimizations may prevent a thunk from
  being created. However, this should not be an issue if the cases are chosen
  carefully. Empirically, this test suite was useful for catching some instances
  of overzealous evaluation early in development of Tvix.

- **identity** test cases require that the given expression evaluates to a
  value whose printed representation is the same (string equal to) the original
  expression. Such test cases only exist in the Tvix test suite.

  Of course only a limited number of expression satisfy this, but it is
  useful for testing `nix-instantiate(1)` style value printing. Consequently,
  it is kind of on the edge of what you can call a language test.

### Extra Dependencies of Some Test Cases

- **Filesystem**: Some test cases `import` other files or use `builtins.readFile`,
  `builtins.readDir` and friends.
- **Working and Home Directory**: Tests involving relative and home relative paths
  need knowledge of the current and home directory to correctly interpret the output.
  C++ Nix does a [search and replace on the test output for this purpose][cpp-nix-pwd-sed]
- **Nix Store**: Some tests add files to the store, either via path interpolation,
  `builtins.toFile` or `builtins.derivation`.

  Additionally, it should be considered that Import-from-Derivation may be
  interesting to test in the future. Currently, the Tvix and C++ Nix test
  suites all pass with Import-from-Derivation disabled, i.e. a dummy store
  implementation is enough.

  Note that the absence of a store dependency ideally also influences the test
  execution: In Tvix, for example, store independent tests can be executed
  with a store backend that immediately errors out, verifying that the test
  is, in fact, store independent.
- **Environment**: The C++ Nix test suite sets a single environment variable,
  `TEST_VAR=foo`. Additionally, `NIX_PATH` and `HOME` are sometimes set (the
  latter is probably not a great idea, since it is not terribly reliable).
- **Nix Path**: A predetermined Nix Path (via `NIX_PATH` and/or command line
  arguments) needs to be set for some test cases.
- **Nix flags**: Some tests need to have extra flags passed to `nix-instantiate(1)`
  in order to work. This is done using a `.flags` file

### Expected Output Considerations

#### Success

The expected output of `eval-okay` test cases (which are the majority of test
cases) uses the standard strict output of `nix-instantiate(1)` in most cases
which is nice to read and easy to work with. However, some more obscure aspects
of this output inevitably leak into the test cases, namely the cycle detection
and printing and (in the case of Tvix) the printing of thunks. Unfortunately,
the output has been changed after Nix 2.3, bringing it closer to the output of
`nix eval`, but in an inconsistent manner (e.g. `<CYCLE>` was changed to
`«repeated»`, but `<LAMBDA>` remained). As a consequence, it is not always
possible to write C++ Nix version independent test cases.

It is unclear whether a satisfying solution (for a common test suite) can
be achieved here as it has become a somewhat contentious [issue whether
or not nix-instantiate should have a stable output](cpp-nix-attr-elision-printing-pr).

A solution may be to use the XML output, specifically the `--xml --no-location`
flags to `nix-instantiate(1)` for some of these instances. As it (hopefully)
corresponds to `builtins.toXML`, there should be a greater incentive to keep it
stable. It does support (only via `nix-instantiate(1)`, though) printing
unevaluated thunks, but has no kind of cycle detection (which is fair enough for
its intended purpose).

#### Failure

C++ Nix has recently (some time after Nix 2.3, probably much later actually)
started checking error messages via expected stderr output. This naturally
won't work for a implementation independent language test suite:

- It is fine to have differing phrasing for error messages or localize them.
- Printed error positions and stack traces may be slightly different depending
  on implementation internals.
- Formatting will almost certainly differ.

Consequently, just checking for failure when running the test suite should be
an option. Long term, it may be interesting to have standardized error codes
and portable error code reporting.

[cpp-nix-pwd-sed]: https://github.com/NixOS/nix/blob/2cb9c7c68102193e7d34fabe6102474fc7f98010/tests/functional/lang.sh#L109
[cpp-nix-attr-elision-printing-pr]: https://github.com/NixOS/nix/pull/9606
