Recursive attribute sets
========================

The construction behaviour of recursive attribute sets is very
specific, and a bit peculiar.

In essence, there are multiple "phases" of scoping that take place
during attribute set construction:

1. Every inherited value without an explicit source is inherited only
   from the **outer** scope in which the attribute set is enclosed.

2. A new scope is opened in which all recursive keys are evaluated.
   This only considers **statically known keys**, attributes can
   **not** recurse into dynamic keys in `self`!

   For example, this code is invalid in C++ Nix:

   ```
   nix-repl> rec { ${"a"+""} = 2; b = a * 10; }
   error: undefined variable 'a' at (string):1:26
   ```

3. Finally, a third scope is opened in which dynamic keys are
   evaluated.

This behaviour, while possibly a bit strange and unexpected, actually
simplifies the implementation of recursive attribute sets in Tvix as
well.

Essentially, a recursive attribute set like this:

```nix
rec {
  inherit a;
  b = a * 10;
  ${"c" + ""} = b * 2;
}
```

Can be compiled like the following expression:

```nix
let
  inherit a;
in let
  b = a * 10;
  in {
    inherit a b;
    ${"c" + ""} = b * 2;
  }
```

Completely deferring the resolution of recursive identifiers to the
existing handling of recursive scopes (i.e. deferred access) in let
bindings.

In practice, we can further specialise this and compile each scope
directly into the form expected by `OpAttrs` (that is, leaving
attribute names on the stack) before each value's position.
