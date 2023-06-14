# Nix language version history

The Nix language (“Nix”) has its own versioning mechanism independent from its
most popular implementation (“C++ Nix”): `builtins.langVersion`. It has been
increased whenever the language has changed syntactically or semantically in a
way that would not be introspectable otherwise. In particular, this does not
include addition (or removal) of `builtins`, as this can be introspected using
standard attribute set operations.

Changes to `builtins.langVersion` are best found by viewing the git history of
C++ Nix using `git log -G 'mkInt\\(v, [0-9]\\)'` for `builtins.langVersion` < 7.
After that point `git log -G 'v\\.mkInt\\([0-9]+\\)'` should work. To reduce the
amount of false positives, specify the version number you are interested in
explicitly.

## 1

The first version of the Nix language is its state at the point when
`builtins.langVersion` was added in [8b8ee53] which was first released
as part of C++ Nix 1.2.

## 2

Nix version 2 changed the behavior of `builtins.storePath`: It would now [try to
substitute the given path if missing][storePath-substitute], instead of creating
an evaluation failure. `builtins.langVersion` was increased in [e36229d].

## 3

Nix version 3 changed the behavior of the `==` behavior. Strings would now be
considered [equal even if they had differing string context][equal-no-ctx].

## 4

Nix version 4 [added the float type][float] to the language.

## 5

The [increase of `builtins.langVersion` to 5][langVersion-5] did not signify a
language change, but added support for structured attributes to the Nix daemon.
Eelco Dolstra writes as to what changed:

> The structured attributes support. Unfortunately that's not so much a language
> change as a build.cc (i.e. daemon) change, but we don't really have a way to
> express that...

Maybe `builtins.nixVersion` (which was added in version 1) should have been
used instead. In any case, the [only `langVersion` check][nixpkgs-langVersion-5]
in nixpkgs verifies a lower bound of 5.

## 6

Nix version 6 added support for [comparing two lists][list-comparison].

[8b8ee53]: https://github.com/nixos/nix/commit/8b8ee53bc73769bb25d967ba259dabc9b23e2e6f
[storePath-substitute]: https://github.com/nixos/nix/commit/22d665019a3770148929b7504c73bcdbe025ec12
[e36229d]: https://github.com/nixos/nix/commit/e36229d27f9ab508e0abf1892f3e8c263d2f8c58
[equal-no-ctx]: https://github.com/nixos/nix/commit/ee7fe64c0ac00f2be11604a2a6509eb86dc19f0a
[float]: https://github.com/nixos/nix/commit/14ebde52893263930cdcde1406cc91cc5c42556f
[langVersion-5]: https://github.com/nixos/nix/commit/8191992c83bf4387b03c5fdaba818dc2b520462d
[list-comparison]: https://github.com/nixos/nix/commit/09471d2680292af48b2788108de56a8da755d661
[nixpkgs-langVersion-5]: https://github.com/NixOS/nixpkgs/blob/d7ac3423d321b8b145ccdd1aed9dfdb280f5e391/pkgs/build-support/closure-info.nix#L11
