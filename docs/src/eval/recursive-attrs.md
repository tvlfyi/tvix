# Recursive attribute sets

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

C++ Nix's Implementation
------------------------

* [`ExprAttrs`](https://github.com/NixOS/nix/blob/2097c30b08af19a9b42705fbc07463bea60dfb5b/src/libexpr/nixexpr.hh#L241-L268)
  (AST representation of attribute sets)
* [`ExprAttrs::eval`](https://github.com/NixOS/nix/blob/075bf6e5565aff9fba0ea02f3333c82adf4dccee/src/libexpr/eval.cc#L1333-L1414)
* [`addAttr`](https://github.com/NixOS/nix/blob/master/src/libexpr/parser.y#L98-L156) (`ExprAttrs` construction in the parser)
