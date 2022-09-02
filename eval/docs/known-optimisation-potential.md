Known Optimisation Potential
============================

There are several areas of the Tvix evaluator code base where
potentially large performance gains can be achieved through
optimisations that we are already aware of.

The shape of most optimisations is that of moving more work into the
compiler to simplify the runtime execution of Nix code. This leads, in
some cases, to drastically higher complexity in both the compiler
itself and in invariants that need to be guaranteed between the
runtime and the compiler.

For this reason, and because we lack the infrastructure to adequately
track their impact (WIP), we have not yet implemented these
optimisations, but note the most important ones here.

* Use "open upvalues" [hard]

  Right now, Tvix will immediately close over all upvalues that are
  created and clone them into the `Closure::upvalues` array.

  Instead of doing this, we can statically determine most locals that
  are closed over *and escape their scope* (similar to how the
  `compiler::scope::Scope` struct currently tracks whether locals are
  used at all).

  If we implement the machinery to track this, we can implement some
  upvalues at runtime by simply sticking stack indices in the upvalue
  array and only copy the values where we know that they escape.

* Avoid `with` value duplication [easy]

  If a `with` makes use of a local identifier in a scope that can not
  close before the with (e.g. not across `LambdaCtx` boundaries), we
  can avoid the allocation of the phantom value and duplication of the
  `NixAttrs` value on the stack. In this case we simply push the stack
  index of the known local.

* Multiple attribute selection [medium]

  An instruction could be introduced that avoids repeatedly pushing an
  attribute set to/from the stack if multiple keys are being selected
  from it. This occurs, for example, when inheriting from an attribute
  set or when binding function formals.

* Split closure/function representation [easy]

  Functions have fewer fields that need to be populated at runtime and
  can directly use the `value::function::Lambda` representation where
  possible.

* Tail-call optimisation [hard]

  We can statically detect the conditions for tail-call optimisation.
  The compiler should do this, and it should then emit a new operation
  for doing the tail-calls.

* Optimise inner builtin access [medium]

  When accessing identifiers like `builtins.foo`, the compiler should
  not go through the trouble of setting up the attribute set on the
  stack and accessing `foo` from it if it knows that the scope for
  `builtins` is unpoisoned.

  The same thing goes for resolving `with builtins;`, which should
  definitely resolve statically.
