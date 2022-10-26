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

* Optimise inner builtin access [medium]

  When accessing identifiers like `builtins.foo`, the compiler should
  not go through the trouble of setting up the attribute set on the
  stack and accessing `foo` from it if it knows that the scope for
  `builtins` is unpoisoned. The same optimisation can also be done
  for the other set operations like `builtins ? foo` and
  `builtins.foo or alternative-implementation`.

  The same thing goes for resolving `with builtins;`, which should
  definitely resolve statically.

* Inline fully applied builtins with equivalent operators [medium]

  Some `builtins` have equivalent operators, e.g. `builtins.add`
  corresponds to the `+` operator, `builtins.hasAttr` to the `?`
  operator etc. These operators additionally compile to a primitive
  VM opcode, so they should be just as cheap (if not cheaper) as
  a builtin application.

  In case the compiler encounters a fully applied builtin (i.e.
  no currying is occurring) and the `builtins` global is unpoisoned,
  it could compile the equivalent operator bytecode instead: For
  example, `builtins.add 20 22` would be compiled as `20 + 22`.
  This would ensure that equivalent `builtins` can also benefit
  from special optimisations we may implement for certain operators
  (in the absence of currying). E.g. we could optimise access
  to the `builtins` attribute set which a call to
  `builtins.getAttr "foo" builtins` should also profit from.

* Avoid nested `VM::run` calls [hard]

  Currently when encountering Nix-native callables (thunks, closures)
  the VM's run loop will nest and return the value of the nested call
  frame one level up. This makes the Rust call stack almost mirror the
  Nix call stack, which is usually undesirable.

  It is possible to detect situations where this is avoidable and
  instead set up the VM in such a way that it continues and produces
  the desired result in the same run loop, but this is kind of tricky
  to get right - especially while other parts are still in flux.

  For details consult the commit with Gerrit change ID
  `I96828ab6a628136e0bac1bf03555faa4e6b74ece`, in which the initial
  attempt at doing this was reverted.

* Avoid thunks if only identifier closing is required [medium]

  Some constructs, like `with`, mostly do not change runtime behaviour
  if thunked. However, they are wrapped in thunks to ensure that
  deferred identifiers are resolved correctly.

  This can be avoided, as we statically analyse the scope and should
  be able to tell whether any such logic was required.

* Intern literals [easy]

  Currently, the compiler emits a separate entry in the constant
  table for each literal.  So the program `1 + 1 + 1` will have
  three entries in its `Chunk::constants` instead of only one.
