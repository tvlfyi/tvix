# (Possible) Implementation(s) of Catchable Errors for `builtins.tryEval`

## Terminology

Talking about “catchable errors” in Nix in general is a bit precarious since
there is no properly established terminology. Also, the existing terms are less
than apt. The reason for this lies in the fact that catchable errors (or
whatever you want to call them) don't properly _exist_ in the language: While
Nix's `builtins.tryEval` is (originally) based on the C++ exception system,
it specifically lacks the ability of such systems to have an exception _value_
whilst handling it. Consequently, these errors don't have an obvious name
as they never appear _in_ the Nix language. They just have to be named in the
respective Nix implementation:

- In C++ Nix the only term for such errors is `AssertionError` which is the
  name of the (C++) exception used in the implementation internally. This
  term isn't great, though, as `AssertionError`s can not only be generated
  using `assert`, but also using `throw` and failed `NIX_PATH` resolutions.
  Were this terminology to be used in documentation addressing Nix language
  users, it would probably only serve confusion.

- Tvix currently (as of r/7573) uses the term catchable errors. This term
  relates to nothing in the language as such: Errors are not caught, we rather
  try to evaluate an expression. Catching also sort of implies that a value
  representation of the error is attainable (like in an exception system) which
  is untrue.

In light of this I (sterni) would like to suggest “tryable errors” as an
alternative term going forward which isn't inaccurate and relates to terms
already established by language internal naming.

However, this document will continue using the term catchable error until the
naming is adjusted in Tvix itself.

## Implementation

Below we discuss different implementation approaches in Tvix in order to arrive
at a proposal for the new one. The historical discussion is intended as a basis
for discussing the proposal: Are we committing to an old or current mistake? Are
we solving all problems that cropped up or were solved at any given point in
time?

### Original

The original implementation of `tryEval` in cl/6924 was quite straightforward:
It would simply interrupt the propagation of a potential catchable error to the
top level (which usually happened using the `?` operator) in the builtin and
construct the appropriate representation of an unsuccessful evaluation if the
error was deemed catchable. It had, however, multiple problems:

- The VM was originally written without `tryEval` in mind, i.e. it largely
  assumed that an error would always cause execution to be terminated. This
  problem was later solved (cl/6940).
- Thunks could not be `tryEval`-ed multiple times (b/281). This was another
  consequence of VM architecture at the time: Thunks would be blackholed
  before evaluation was started and the error could occur. Due to the
  interaction of the generator-based VM code and `Value::force` the part
  of the code altering the thunk state would never be informed about the
  evaluation result in case of a failure, so the thunk would remain
  blackholed leading to a crash if the same thunk was `tryEval`-ed or
  forced again. To solve this issue, amjoseph completely overhauled
  the implementation.

One key point about this implementation is that it is based on the assumption
that catchable errors can only be generated in thunks, i.e. expressions causing
them are never evaluated strictly. This can be illustrated using C++ Nix:

```console
> nix-instantiate --eval -E '[ (assert false; true) (builtins.throw "") <nixpkgs> ]'
[ <CODE> <CODE> <CODE> ]
```

If this wasn't the case, the VM could encounter the error in a situation where
the error would not have needed to pass through the `tryEval` builtin, causing
evaluation to abort.

### Present

The current system (mostly implemented in cl/9289) uses a very different
approach: Instead of relying on the thunk boundary, catchable errors are no
longer errors, but special values. They are created at the relevant points (e.g.
`builtins.throw`) and propagated whenever they are encountered by VM ops or
builtins. Finally, they either encounter `builtins.tryEval` (and are converted to
an ordinary value again) or the top level where they become a normal error again.

The problems with this mostly stem from the confusion between values and errors
that it necessitates:

- In most circumstances, catchable errors end up being errors again, as `tryEval`
  is not used a lot. So `throw`s usually end up causing evaluation to abort.
  Consequently, not only `Value::Catchable` is necessary, but also a corresponding
  error variant that is _only_ created if a catchable value remains at the end of
  evaluation. A requirement that was missed until cl/10991 (!) which illustrate
  how strange that architecture is. A consequence of this is that catchable
  errors have no location information at all.
- `Value::Catchable` is similar to other internal values in Tvix, but is much
  more problematic. Aside from thunks, internal values only exist for a brief
  amount of time on the stack and it is very clear what parts of the VM or
  builtins need to handle them. This means that the rest of the implementation
  need to consider them, keeping the complexity caused by the internal value
  low. `Value::Catchable`, on the other hand, may exist anywhere and be passed
  to any VM op or builtin, so it needs to be correctly propagated _everywhere_.
  This causes a lot of noise in the code as well as a big potential for bugs.
  Essentially, catchable errors require as much attention by the Tvix developer
  as laziness. This doesn't really correlate to the importance of the two
  features to the Nix language.

### Future?

The core assumption of the original solution does offer a path forward: After
cl/9289 we should be in a better position to introspect an error occurring from
within the VM code, but we need a better way of storing such an error to prevent
another b/281. If catchable errors can only be generated in thunks, we can just
use the thunk representation for this. This would mean that `Thunk::force_`
would need to check if evaluation was successful and (in case of failure)
change the thunk representation

- either to the original `ThunkRepr::Suspended` which would be simple, but of
  course mean duplicated evaluation work in some expressions. In fact, this
  would probably leave a lot of easy performance on the table for use cases we
  would like to support, e.g. tree walkers for nixpkgs.
- or to a new `ThunkRepr` variant that stores the kind of the error and all
  necessary location info so stack traces can work properly. This of course
  reintroduces some of the difficulty of having two kinds of errors, but it is
  hopefully less problematic, as the thunk boundary (i.e. `Thunk::force`) is
  where errors would usually occur.

Besides the question whether this proposal can actually be implemented, another
consideration is whether the underlying assumption will hold in the future, i.e.
can we implement optimizations for thunk elimination in a way that thunks that
generate catchable errors are never eliminated?
