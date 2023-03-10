# Value Pointer Equality in Nix

## Introduction

It is a piece of semi-obscure Nix trivia that while functions are generally not
comparable, they can be compared in certain situations. This is actually quite an
important fact, as it is essential for the evaluation of nixpkgs: The attribute sets
used to represent platforms in nixpkgs, like `stdenv.buildPlatform`, contain functions,
such as `stdenv.buildPlatform.canExecute`. When writing cross logic, one invariably
ends up writing expressions that compare these sets, e.g. `stdenv.buildPlatform !=
stdenv.hostPlatform`. Since attribute set equality is the equality of their attribute
names and values, we also end up comparing the functions within them.  We can summarize
the relevant part of this behavior for platform comparisons in the following (true)
Nix expressions:

* `stdenv.hostPlatform.canExecute != stdenv.hostPlatform.canExecute`
* `stdenv.hostPlatform == stdenv.hostPlatform`

This fact is commonly referred to as pointer equality of functions (or function pointer
equality) which is not an entirely accurate name, as we'll see. This account of the
behavior states that, while functions are incomparable in general, they are comparable
insofar, as they occupy the same spot in an attribute set.

However, [a maybe lesser known trick][puck-issue] is to write a function such as the
following to allow comparing functions:

```nix
let
  pointerEqual = lhs: rhs: { x = lhs; } == { x = rhs; };

  f = name: "Hello, my name is ${name}";
  g = name: "Hello, my name is ${name}";
in
[
  (pointerEqual f f) # => true
  (pointerEqual f g) # => false
]
```

Here, clearly, the function is not contained at the same position in one and the same
attribute set, but at the same position in two entirely different attribute sets. We can
also see that we are not comparing the functions themselves (e.g. their AST), but
rather if they are the same individual value (i.e. pointer equal).

So what is _actually_ going on?

## Nix (pointer) Equality in C++ Nix

TIP: The summary presented here is up-to-date as of 2022-11-23 and was tested with Nix 2.3 and 2.11.

The function implementing equality in C++ Nix is `EvalState::eqValues` which starts with
[the following bit of code][eqValues-pointer-eq]:

```cpp
bool EvalState::eqValues(Value & v1, Value & v2)
{
    forceValue(v1);
    forceValue(v2);

    /* !!! Hack to support some old broken code that relies on pointer
       equality tests between sets.  (Specifically, builderDefs calls
       uniqList on a list of sets.)  Will remove this eventually. */
    if (&v1 == &v2) return true;
```

So this immediately looks more like pointer equality of arbitrary *values* instead of functions. In fact
there is [no special code facilitating function equality][eqValues-function-eq]:

```cpp
        /* Functions are incomparable. */
        case nFunction:
            return false;
```

So one takeaway of this is that pointer equality is neither dependent on functions nor attribute sets.
In fact, we can also write our `pointerEqual` function as:

```nix
lhs: rhs: [ lhs ] == [ rhs ]
```

It's interesting that `EvalState::eqValues` forces the left and right-hand value before trying pointer
equality. It explains that `let x = throw ""; in x == x` does not evaluate successfully, but it is puzzling why
`let f = x: x; in f == f` does not return `true`. In fact, why do we need to wrap the values in a list or
attribute set at all for our `pointerEqual` function to work?

The answer lies in [the code that evaluates `ExprOpEq`][ExprOpEq],
i.e. an expression involving the `==` operator:

```cpp
void ExprOpEq::eval(EvalState & state, Env & env, Value & v)
{
    Value v1; e1->eval(state, env, v1);
    Value v2; e2->eval(state, env, v2);
    v.mkBool(state.eqValues(v1, v2));
}
```

As you can see, two _distinct_ `Value` structs are created, so they can never be pointer equal even
if the `union` inside points to the same bit of memory. We can thus understand what actually happens
when we check the equality of an attribute set (or list), by looking at the following expression:

```nix
let
  x = { name = throw "nameless"; };
in

x == x # => causes an evaluation error
```

Because `x` can't be pointer equal, as it'll end up in the distinct structs `v1` and `v2`, it needs to be compared
by value. For this reason, the `name` attribute will be forced and an evaluation error caused.
If we rewrite the expression to use???

```nix
{ inherit x; } == { inherit x; } # => true
```

???, it'll work: The two attribute sets are compared by value, but their `x` attribute turns out to be pointer
equal _after_ forcing it. This does not throw, since forcing an attribute set does not force its attributes'
values (as forcing a list doesn't force its elements).

As we have seen, pointer equality can not only be used to compare function values, but also other
otherwise incomparable values, such as lists and attribute sets that would cause an evaluation
error if they were forced recursively. We can even switch out the `throw` for an `abort`. The limitation is
of course that we need to use a value that behaves differently depending on whether it is forced
???normally??? (think `builtins.seq`) or recursively (think `builtins.deepSeq`), so thunks will generally be
evaluated before pointer equality can kick into effect.

## Summary

When comparing two Nix values, we must force both of them (non-recursively!), but are
allowed to short-circuit the comparison based on pointer equality, i.e. if they are at
the same exact value in memory, they are deemed equal immediately. This is completely
independent of what type of value they are. If they are not pointer equal, they are
(recursively) compared by value as expected.

However, when evaluating the Nix expression `a == b`, we *must* invoke our implementation's
value equality function in a way that `a` and `b` themselves can never be deemed pointer equal.
Any values we encounter while recursing during the equality check must be compared by
pointer as described above, though.

## Other Comparisons

The `!=` operator uses `EvalState::eqValues` internally as well, so it behaves exactly as `!(a == b)`.

The `>`, `<`, `>=` and `<=` operators all desugar to [CompareValues][] eventually
which generally looks at the value type before comparing. It does, however, rely on
`EvalState::eqValues` for list comparisons, so it is possible to compare lists with
e.g. functions in them, as long as they are equal by pointer:

```nix
let
  f = x: x + 42;
in

[
  ([ f 2 ] > [ f 1 ]) # => true
  ([ f 2 ] > [ (x: x) 1]) # => error: cannot compare a function with a function
]
```

Finally, since `builtins.elem` relies on `EvalState::eqValues`, you can check for
a function by pointer equality:

```nix
let
  f = x: f x;
in
builtins.elem f [ f 2 3 ] # => true
```

## Stability of the Feature

Keen readers will have noticed the following comment in the C++ Nix source code,
indicating that pointer comparison may be removed in the future.

```cpp
    /* !!! Hack to support some old broken code that relies on pointer
       equality tests between sets.  (Specifically, builderDefs calls
       uniqList on a list of sets.)  Will remove this eventually. */
```

Now, I can't speak for the upstream C++ Nix developers, but sure can speculate.
As already pointed out, this feature is currently needed for evaluating nixpkgs.
While its use could realistically be eliminated (only bothersome spot is probably
the `emulator` function, but that should also be doable), removing the feature
would seriously compromise C++ Nix's ability to evaluate historical nixpkgs
revision which is arguably a strength of the system.

Another indication that it is likely here to stay is that it has already
[outlived builderDefs][], even though
it was (apparently) reintroduced just for this use case. More research into
the history of this feature would still be prudent, especially the reason for
its original introduction (maybe performance?).

[puck-issue]: https://github.com/NixOS/nix/issues/3371
[eqValues-pointer-eq]: https://github.com/NixOS/nix/blob/05d0892443bbe92a6b6a1ee7b1d37ea05782d918/src/libexpr/eval.cc#L2342-L2350
[eqValues-function-eq]: https://github.com/NixOS/nix/blob/05d0892443bbe92a6b6a1ee7b1d37ea05782d918/src/libexpr/eval.cc#L2405-L2407
[ExprOpEq]: https://github.com/NixOS/nix/blob/05d0892443bbe92a6b6a1ee7b1d37ea05782d918/src/libexpr/eval.cc#L1856-L1861
[outlived builderDefs]: https://github.com/NixOS/nixpkgs/issues/4210
[CompareValues]: https://github.com/NixOS/nix/blob/master/src/libexpr/primops.cc#L536-L574
