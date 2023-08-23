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

* Apply `compiler::optimise_select` to other set operations [medium]

  In addition to selects, statically known attribute resolution could
  also be used for things like `?` or `with`. The latter might be a
  little more complicated but is worth investigating.

* Inline fully applied builtins with equivalent operators [medium]

  Some `builtins` have equivalent operators, e.g. `builtins.sub`
  corresponds to the `-` operator, `builtins.hasAttr` to the `?`
  operator etc. These operators additionally compile to a primitive
  VM opcode, so they should be just as cheap (if not cheaper) as
  a builtin application.

  In case the compiler encounters a fully applied builtin (i.e.
  no currying is occurring) and the `builtins` global is unshadowed,
  it could compile the equivalent operator bytecode instead: For
  example, `builtins.sub 20 22` would be compiled as `20 - 22`.
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

* Do some list and attribute set operations in place [hard]

  Algorithms that can not do a lot of work inside `builtins` like `map`,
  `filter` or `foldl'` usually perform terribly if they use data structures like
  lists and attribute sets.

  `builtins` can do work in place on a copy of a `Value`, but naïvely expressed
  recursive algorithms will usually use `//` and `++` to do a single change to a
  `Value` at a time, requiring a full copy of the data structure each time.
  It would be a big improvement if we could do some of these operations in place
  without requiring a new copy.

  There are probably two approaches: We could determine statically if a value is
  reachable from elsewhere and emit a special in place instruction if not. An
  easier alternative is probably to rely on reference counting at runtime: If no
  other reference to a value exists, we can extend the list or update the
  attribute set in place.

  An **alternative** to this is using [persistent data
  structures](https://en.wikipedia.org/wiki/Persistent_data_structure) or at the
  very least [immutable data structures](https://docs.rs/im/latest/im/) that can
  be copied more efficiently than the stock structures we are using at the
  moment.

* Skip finalising unfinalised thunks or non-thunks instead of crashing [easy]

  Currently `OpFinalise` crashes the VM if it is called on values that don't
  need to be finalised. This helps catching miscompilations where `OpFinalise`
  operates on the wrong `StackIdx`. In the case of function argument patterns,
  however, this means extra VM stack and instruction overhead for dynamically
  determining if finalisation is necessary or not. This wouldn't be necessary
  if `OpFinalise` would just noop on any values that don't need to be finalised
  (anymore).

* Phantom binding for from expression of inherits [easy]

  The from expression of an inherit is reevaluated for each inherit. This can
  be demonstrated using the following Nix expression which, counter-intuitively,
  will print “plonk” twice.

  ```nix
  let
    inherit (builtins.trace "plonk" { a = null; b = null; }) a b;
  in
  builtins.seq a (builtins.seq b null)
  ```

  In most Nix code, the from expression is just an identifier, so it is not
  terribly inefficient, but in some cases a more expensive expression may
  be used. We should create a phantom binding for the from expression that
  is reused in the inherits, so only a single thunk is created for the from
  expression.
