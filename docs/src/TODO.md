# TODO

This contains a rough collection of ideas on the TODO list, trying to keep track
of it somewhere.

Of course, there's no guarantee these things will get addressed, but it helps
dumping the backlog somewhere.

Feel free to add new ideas. Before picking something, ask in `#tvix-dev` to make
sure noone is working on this, or has some specific design in mind already.

## Cleanups
### Nix language test suite
 - Think about how to merge, but "categorize" `tvix_tests` in `glue` and `eval`.
   We currently only have this split as they need a different feature set /
   builtins.
 - move some of the rstest cases in `tvix-glue` to the `.nix`/`.exp` mechanism.
   Some of them need test fixtures, which cannot be represented in git (special
   file types in the import tests for example). Needs some support from the test
   suite to create these fixtures on demand.
 - extend `verify-lang-tests/default.nix` mechanism to validate `tvix-eval` and
   `tvix-glue` test cases (or the common structure above).
 - absorb `eval/tests/nix_oracle.rs` into `tvix_tests`, or figure out why it's
   not possible (and document) it. It looks like it's only as nix is invoked
   with a different level of `--strict`, but the toplevel doc-comment suggests
   its generic?

### Error cleanup
 - Currently, all services use tvix_castore::Error, which only has two kinds
   (invalid request, storage error), containing an (owned) string.
   This is quite primitive. We should have individual error types for BS, DS, PS.
   Maybe these should have some generics to still be able to carry errors from
   the underlying backend, similar to `IngestionError`.

## Fixes towards correctness
 - `builtins.toXML` is missing string context. See b/398.
 - `builtins.toXML` self-closing tags need to be configurable in a more granular
   fashion, requires third-party crate support. See b/399.
 - `rnix` only supports string source files, but `NixString` uses bytes (and Nix
   source code might be no valid UTF-8).

## Documentation
Extend the other pages in here. Some ideas on what should be tackled:
 - Document what Tvix is, and what it is not yet. What it is now, what it is not
   (yet), explaining some of the architectural choices (castore, more hermetic
   `Build` repr), while still being compatible. Explain how it's possible to
   plug in other frontends, and use `tvix-{[ca]store,build}` without Nixlang even.
   And how `nix-compat` is a useful crate for all sorts of formats and data
   types of Nix.
 - Update the Architecture diagram to model the current state of things.
   There's no gRPC between Coordinator and Evaluator.
 - Add a dedicated section/page explaining the separation between tvix-glue and
   tvix-eval, and how more annoying builtins get injected into tvix-eval through
   tvix-glue.
   Maybe restructure to only explain the component structure potentially
   crossing process boundaries (those with gRPC), and make the rest more crate
   and trait-focused?
 - Restructure docs on castore vs store, this seems to be duplicated a bit and
   is probably still not too clear.
 - Describe store composition(s) in more detail. There's some notes on granular
   fetching which probably can be repurposed.
 - Absorb the rest of //tvix/website into this.

## Features

### CLI
 - `nix repl` can set variables and effectively mutates a global scope. We
  should update the existing / add another repl that allows the same. We don't
  want to mutate the evaluator, but should construct a new one, passing in the
  root scope returned from the previous evaluation.

### Fetchers
Some more fetcher-related builtins need work:
 - `fetchGit`
 - `fetchTree` (hairy, seems there's no proper spec and the URL syntax seems
   subject to change/underdocumented)

### Convert builtins:fetchurl to Fetches
We need to convert `builtins:fetchurl`-style calls to `builtins.derivation` to
fetches, not Derivations (tracked in `KnownPaths`).

### Derivation -> Build
While we have some support for `structuredAttrs` and `fetchClosure` (at least
enough to calculate output hashes, aka produce identical ATerm), the code
populating the `Build` struct doesn't exist it yet.

Similarly, we also don't properly populate the build environment for
`fetchClosure` yet. (Note there already is `ExportedPathInfo`, so once
`structuredAttrs` is there this should be easy.

### Builders
Once builds are proven to work with real-world builds, and the corner cases
there are ruled out, adding other types of builders might be interesting.

 - bwrap
 - gVisor
 - Cloud Hypervisor (using similar technique as `//tvix//boot`).

Long-term, we want to extend traits and gRPC protocol to expose more telemetry,
logs etc, but this is something requiring a lot of designing.

### Store composition
 - Combinators: list-by-priority, first-come-first-serve, cache
 - How do describe hierarchies. URL format too one-dimensional, but we might get
   quite far with a similar "substituters" concept that Nix uses, to construct
   the composed stores.
### Store Config
   There's already serde for some store options (bigtable uses `serde_qs`).
   We might also have common options global over all backends, like chunking
   parameters for chunking blobservices. Think where this would fit in.
 - Rework the URL syntax for object_store. We should support the default s3/gcs
   URLs at least.

### BlobService
 - On the trait side, currently there's no way to distinguish reading a
   known-chunk vs blob, so we might be calling `.chunks()` unnecessarily often.
   At least for the `object_store` backend, this might be a problem.
 - While `object_store` recently got support for `Content-Type`
   (https://github.com/apache/arrow-rs/pull/5650), there's no support on the
   local filesystem yet. We'd need to add support to this (through xattrs).

### DirectoryService
 - Add an `object_store` variant, storing a Directory *closure* keyed by the
   root `Directory` digest. This won't allow indexing intermediate Directory
   nodes, but once we have `DirectoryService` composition, it shouldn't be an
   issue.
 - [redb](https://www.redb.org/) backend

### PathInfoService
 - [redb](https://www.redb.org/) backend
 - sqlite backend (different schema than the Nix one, we need the root nodes data!)

### Nix-compat
- Async NAR reader (@edef?)

### Nix Daemon protocol
- Some work ongoing on the worker operation parsing. Partially blocked on the async NAR reader.

### O11Y
 - gRPC trace propagation (cl/10532)
 - `tracing-tracy` (cl/10952)
 - `[tracing-]indicatif` for progress/log reporting (floklis stash)
 - unification into `tvix-tracing` crate, currently a lot of boilerplate
   in `tvix-store` CLI entrypoint, and half of the boilerplate copied over to
   `tvix-cli`.
