Build references in derivations
===============================

This document describes how build references are calculated in Tvix. Build
references are used to determine which store paths should be available to a
builder during the execution of a build (i.e. the full build closure of a
derivation).

## String contexts in C++ Nix

In C++ Nix, each string value in the evaluator carries an optional so-called
"string context".

These contexts are themselves a list of strings that take one of the following
formats:

1. `!<output_name>!<drv_path>`

   This format describes a build reference to a specific output of a derivation.

2. `=<drv_path>`

   This format is used for a special case where a derivation attribute directly
   refers to a derivation path (e.g. by accessing `.drvPath` on a derivation).

   Note: In C++ Nix this case is quite special and actually requires a
   store-database query during evaluation.

3. `<path>` - a non-descript store path input, usually a plain source file (e.g.
   from something like `src = ./.` or `src = ./foo.txt`).

   In the case of `unsafeDiscardOutputDependency` this is used to pass a raw
   derivation file, but *not* pull in its outputs.

Lets introduce names for these (in the same order) to make them easier to
reference below:

```rust
enum BuildReference {
    /// !<output_name>!<drv_path>
    SingleOutput(OutputName, DrvPath),

    /// =<drv_path>
    DrvClosure(DrvPath),

    /// <path>
    Path(StorePath),
}
```

String contexts are, broadly speaking, created whenever a string is the result
of a computation (e.g. string interpolation) that used a *computed* path or
derivation in any way.

Note: This explicitly does *not* include simply writing a literal string
containing a store path (whether valid or not). That is only permitted through
the `storePath` builtin.

## Derivation inputs

Based on the data above, the fields `inputDrvs` and `inputSrcs` of derivations
are populated in `builtins.derivationStrict` (the function which
`builtins.derivation`, which isn't actually a builtin, wraps).

`inputDrvs` is represented by a map of derivation paths to the set of their
outputs that were referenced by the context.

TODO: What happens if the set is empty? Somebody claimed this means all outputs.

`inputSrcs` is represented by a set of paths.

These are populated by the above references as follows:

* `SingleOutput` entries are merged into `inputDrvs`
* `Path` entries are inserted into `inputSrcs`
* `DrvClosure` leads to a special store computation (`computeFSClosure`), which
  finds all paths referenced by the derivation and then inserts all of them into
  the fields as above (derivations with _all_ their outputs)

This is then serialised in the derivation and passed down the pipe.

## Builtins interfacing with contexts

C++ Nix has several builtins that interface directly with string contexts:

* `unsafeDiscardStringContext`: throws away a string's string context (if
  present)
* `hasContext`: returns `true`/`false` depending on whether the string has
  context
* `unsafeDiscardOutputDependency`: drops dependencies on the *outputs* of a
  `.drv` in the context, passing only the literal `.drv` itself

  Note: This is only used for special test-cases in nixpkgs, and deprecated Nix
  commands like `nix-push`.
* `getContext`: returns the string context in serialised form as a Nix attribute
  set
* `appendContext`: adds a given string context to the string in the same format
  as returned by `getContext`

## Placeholders

C++ Nix has `builtins.placeholder`, which given the name of an output (e.g.
`out`) creates a hashed string representation of that output name. If that
string is used anywhere in input attributes, the builder will replace it with
the actual name of the corresponding output of the current derivation.

C++ Nix does not use contexts for this, it blindly creates a rewrite map of
these placeholder strings to the names of all outputs, and runs the output
replacement logic on all environment variables it creates, attribute files it
passes etc.

## Tvix & string contexts

Tvix does not track string contexts in its evaluator at all. Instead we are
investigating implementing a system which allows us to drop string contexts in
favour of reference scanning derivation attributes.

This means that instead of maintaining and passing around a string context data
structure in eval, we maintain a data structure of *known paths* from the same
evaluation elsewhere in Tvix, and scan each derivation attribute against this
set of known paths when instantiating derivations.

Until proven otherwise, we take the stance that the system of string contexts as
implemented in C++ Nix is likely an implementation detail that should not be
leaking to the language surface as it does now.

### Tracking "known paths"

Every time a Tvix evaluation does something that causes a store interaction, a
"known path" is created. On the language surface, this is the result of one of:

1. Path literals (e.g. `src = ./.`).
2. Calls to `builtins.derivationStrict` yielding a derivation and its output
   paths.
3. Calls to `builtins.path`.

Whenever one of these occurs, some metadata that persists for the duration of
one evaluation should be created in Nix. This metadata needs to be available in
`builtins.derivationStrict`, and should be able to respond to these queries:

1. What is the set of all known paths? (used for e.g. instantiating an
   Aho-Corasick type string searcher)
2. What is the _type_ of a path? (derivation path, derivation output, source
   file)
3. What are the outputs of a derivation?
4. What is the derivation of an output?

These queries will need to be asked of the metadata when populating the
derivation fields.

Note: Depending on how we implement `builtins.placeholder`, it might be useful
to track created placeholders in this metadata, too.

### Context builtins

Context-reading builtins can be implemented in Tvix by adding `hasContext` and
`getContext` with the appropriate reference-scanning logic. However, we should
evaluate how these are used in nixpkgs and whether their uses can be removed.

Context-mutating builtins can be implemented by tracking their effects in the
value representation of Tvix, however we should consider not doing this at all.

`unsafeDiscardOutputDependency` should probably never be used and we should warn
or error on it.

`unsafeDiscardStringContext` is often used as a workaround for avoiding IFD in
inconvenient places (e.g. in the TVL depot pipeline generation). This is
unnecessary in Tvix. We should evaluate which other uses exist, and act on them
appropriately.

The initial danger with diverging here is that we might cause derivation hash
discrepancies between Tvix and C++ Nix, which can make initial comparisons of
derivations generated by the two systems difficult. If this occurs we need to
discuss how to approach it, but initially we will implement the mutating
builtins as no-ops.
