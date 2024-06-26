# Compilation of bindings

Compilation of Nix bindings is one of the most mind-bending parts of Nix
evaluation. The implementation of just the compilation is currently almost 1000
lines of code, excluding the various insane test cases we dreamt up for it.

## What is a binding?

In short, any attribute set or `let`-expression. Tvix currently does not treat
formals in function parameters (e.g. `{ name ? "fred" }: ...`) the same as these
bindings.

They have two very difficult features:

1. Keys can mutually refer to each other in `rec` sets or `let`-bindings,
   including out of definition order.
2. Attribute sets can be nested, and parts of one attribute set can be defined
   in multiple separate bindings.

Tvix resolves as much of this logic statically (i.e. at compile-time) as
possible, but the procedure is quite complicated.

## High-level concept

The idea behind the way we compile bindings is to fully resolve nesting
statically, and use the usual mechanisms (i.e. recursion/thunking/value
capturing) for resolving dynamic values.

This is done by compiling bindings in several phases:

1. An initial compilation phase *only* for plain inherit statements (i.e.
   `inherit name;`), *not* for namespaced inherits (i.e. `inherit (from)
   name;`).

2. A declaration-only phase, in which we use the compiler's scope tracking logic
   to calculate the physical runtime stack indices (further referred to as
   "stack slots" or just "slots") that all values will end up in.

   In this phase, whenever we encounter a nested attribute set, it is merged
   into a custom data structure that acts like a synthetic AST node.

   This can be imagined similar to a rewrite like this:

   ```nix
   # initial code:
   {
       a.b = 1;
       a.c = 2;
   }

   # rewritten form:
   {
       a = {
           b = 1;
           c = 2;
       };
   }
   ```

   The rewrite applies to attribute sets and `let`-bindings alike.

   At the end of this phase, we know the stack slots of all namespaces for
   inheriting from, all values inherited from them, and all values (and
   optionally keys) of bindings at the current level.

   Only statically known keys are actually merged, so any dynamic keys that
   conflict will lead to a "key already defined" error at runtime.

3. A compilation phase, in which all values (and, when necessary, keys) are
   actually compiled. In this phase the custom data structure used for merging
   is encountered when compiling values.

   As this data structure acts like an AST node, the process begins recursively
   for each nested attribute set.

At the end of this process we have bytecode that leaves the required values (and
optionally keys) on the stack. In the case of attribute sets, a final operation
is emitted that constructs the actual attribute set structure at runtime. For
`let`-bindings a final operation is emitted that removes these locals from the
stack when the scope ends.

## Moving parts

```admonish caution
This documents the *current* implementation. If you only care about the
conceptual aspects, see above.
```

There's a few types involved:

* `PeekableAttrs`: peekable iterator over an attribute path (e.g. `a.b.c`)
* `BindingsKind`: enum defining the kind of bindings (attrs/recattrs/let)
* `AttributeSet`: struct holding the bindings kind, the AST nodes with inherits
  (both namespaced and not), and an internal representation of bindings
  (essentially a vector of tuples of the peekable attrs and the expression to
  compile for the value).
* `Binding`: enum describing the kind of binding (namespaced inherit, attribute
  set, plain binding of *any other value type*)
* `KeySlot`: enum describing the location in which a key slot is placed at
  runtime (nowhere, statically known value in a slot, dynamic value in a slot)
* `TrackedBinding`: struct representing statically known information about a
  single binding (its key slot, value slot and `Binding`)
* `TrackedBindings`: vector of tracked bindings, which implements logic for
  merging attribute sets together

And quite a few methods on `Compiler`:

* `compile_bindings`: entry point for compiling anything that looks like a
  binding, this calls out to the functions below.
* `compile_plain_inherits`: takes all inherits of a bindings node and compiles
  the ones that are trivial to compile (i.e. just plain inherits without a
  namespace). The `rnix` parser does not represent namespaced/plain inherits in
  different nodes, so this function also aggregates the namespaced inherits and
  returns them for further use
* `declare_namespaced_inherits`: passes over all namespaced inherits and
  declares them on the locals stack, as well as inserts them into the provided
  `TrackedBindings`
* `declare_bindings`: declares all regular key/value bindings in a bindings
  scope, but without actually compiling their keys or values.

  There's a lot of heavy lifting going on here:

  1. It invokes the various pieces of logic responsible for merging nested
     attribute sets together, creating intermediate data structures in the value
     slots of bindings that can be recursively processed the same way.
  2. It decides on the key slots of expressions based on the kind of bindings,
     and the type of expression providing the key.
* `bind_values`: runs the actual compilation of values. Notably this function is
  responsible for recursively compiling merged attribute sets when it encounters
  a `Binding::Set` (on which it invokes `compile_bindings` itself).

In addition to these several methods (such as `compile_attr_set`,
`compile_let_in`, ...) invoke the binding-kind specific logic and then call out
to the functions above.
