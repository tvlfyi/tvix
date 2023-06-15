# Nix language issues

In the absence of a language standard, what Nix (the language) is, is prescribed
by the behavior of the C++ Nix implementation. Still, there are reasons not to
accept some behavior:

* Tvix aims for nixpkgs compatibility only. This means we can ignore behavior in
  edge cases nixpkgs doesn't trigger as well as obscure features it doesn't use
  (e.g. `__overrides`).
* Some behavior of the Nix evaluator seems to be unintentional or an
  implementation detail leaking out into language behavior.

Especially in the latter case, it makes sense to raise the respective issue and
maybe to get rid of the behavior in all implementations for good. Below is an
(incomplete) list of such issues:

* [Behaviour of nested attribute sets depends on definition order][i7111]
* [Partially constructed attribute sets are observable during dynamic attr names construction][i7012]
* [Nix parsers merges multiple attribute set literals for the same key incorrectly depending on definition order][i7115]

On the other hand, there is behavior that seems to violate one's expectation
about the language at first, but has good enough reasons from an implementor's
perspective to keep them:

* Dynamic keys are forbidden in `let` and `inherit`. This makes sure that we
  only need to do runtime identifier lookups for `with`. More dynamic (i.e.
  runtime) lookups would make the scoping system even more complicated as well
  as hurt performance.
* Dynamic attributes of `rec` sets are not added to its scope. This makes sense
  for the same reason.
* Dynamic and nested attributes in attribute sets don't get merged. This is a
  tricky one, but avoids doing runtime (recursive) merges of attribute sets.
  Instead all necessary merging can be inferred statically, i.e. the C++ Nix
  implementation already merges at parse time, making nested attribute keys
  syntactic sugar effectively.

Other behavior is just odd, surprising or underdocumented:

* `builtins.foldl'` doesn't force the initial accumulator (but all other
  intermediate accumulator values), differing from e.g. Haskell, see
  the [relevant PR discussion][p7158].

[i7111]: https://github.com/NixOS/nix/issues/7111
[i7012]: https://github.com/NixOS/nix/issues/7012
[i7115]: https://github.com/NixOS/nix/issues/7115
[p7158]: https://github.com/NixOS/nix/pull/7158
