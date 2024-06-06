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

### crate2nix for WASM
Most of Tvix is living inside a `//tvix` cargo workspace, and we use `crate2nix`
as a build system, to get crate-level build granularity (and caching), keeping
compile times somewhat manageable.

In the future, for Store/Build, we want to build some more web frontends,
exposing some data by calling to the API. Being able to write this in Rust,
and reusing most of our existing code dealing with the data structures would
be preferred.

However, using the crate2nix tooling in combination with compiling for WASM is
a bumpy ride (and `//web.tvixbolt` works around this by using
`rustPlatform.buildRustPackage` instead, which invokes cargo inside a FOD):

`buildRustCrate` in nixpkgs (which is used by `crate2nix` under the hood)
doesn't allow specifying another `--target` explicitly, but relies on the cross
machinery in nixpkgs exclusively.

`doc/languages-frameworks/rust.section.md` suggests it should be a matter of
re-instantiating nixpkgs for `wasm32-unknown-unknown`, but that's no recognized
as a valid architecture.
The suggested alternative, setting only `rustc.config` to it seems to get us
further, but the `Crate.nix` logic for detecting arch-conditional crates doesn't
seem to cover that case, and tries to build crates (`cpufeatures` for `sha{1,2}`)
which are supposed to be skipped.

## Perf
 - String Contexts currently do a lot of indirections (edef)
   (NixString -> NixStringInner -> HashSet[element] -> NixContextElement -> String -> data)
   to get to the actual data. We should improve this. There's various ideas, one
   of it is globally interning all Nix context elements, and only keeping
   indices into that. We might need to have different representations for small
   amount of context elements or larger ones, and need tooling to reason about
   the amount of contexts we have.

### Error cleanup
 - Currently, all services use tvix_castore::Error, which only has two kinds
   (invalid request, storage error), containing an (owned) string.
   This is quite primitive. We should have individual error types for BS, DS, PS.
   Maybe these should have some generics to still be able to carry errors from
   the underlying backend, similar to `IngestionError`.
   There was an attempt to give PS separate error types (cl/11695), but this
   ended up very verbose.
   Every error had to be boxed, and a possible additional message be added. Some
   errors that didn't wrap another underlying errors were hard to construct, too
   (requiring the addition of errors). All of this without even having added
   proper backtrace support, which would be quite helpful in store hierarchies.
   `anyhow`'s `.context()` gives us most of this out of the box. Maybe we can
   use that, using enums rather than `&'static str` as context in some cases?

## Fixes towards correctness
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
 - Store composition hierarchies (@yuka).
   - URL format too one-dimensional.
   - We want to have nice and simple user-facing substituter config, including
     sensible default wrappers for caching, retries, fallbacks, as well as
     granular control for power-users.
   - Current design idea:
     - Have a concept similar to rclone config (map with store aliases as
       keys, allowing to refer to stores by their alias from other parts of
       the config).
       It allows both referring to by name, as well as ad-hoc definition:
       https://rclone.org/docs/#syntax-of-remote-paths
     - Each store needs to be aware of its "instance name", so it can be
       included in logs, metrics, …
     - Have a "instantiation function" traversing such a config data structure,
       creating store instances and plugging them together, ultimately returning
       a dyn …Service interface.
     - No reconfiguration/reconcilation for now
     - Making URLs the primary data format would get ugly quite easily (hello
       multiple layers of escaping!), so best to convert the existing URL
       syntax to our new config format on the fly and then use one codepath
       to instantiate/assemble. Similarly, something like the "user-facing
       substituter config" mentioned above could aalso be converted to such a
       config format under the hood.
     - Maybe add a ?cache=$other_url parameter support to the URL syntax, to
       easily wrap a store with a caching frontend, using $other_url as the
      "near" store URL.

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

### Nix Daemon protocol
- Some work ongoing on the worker operation parsing (griff, picnoir)

### O11Y
 - Maybe drop `--log-level` entirely, and only use `RUST_LOG` env exclusively?
   `debug`,`trace` level across all crates is a bit useless, and `RUST_LOG` can
   be much more granular…
 - The OTLP stack is quite spammy if there's no OTLP collector running on
   localhost.
   https://opentelemetry.io/docs/specs/otel/configuration/sdk-environment-variables/
   mentions a `OTEL_SDK_DISABLED` env var, but it defaults to false, so they
   suggest enabling OTLP by default.
   We currently have a `--otlp` cmdline arg which explicitly needs to be set to
   false to stop it, in line with that "enabled by default" philosophy
   Do some research if we can be less spammy. While OTLP support is
   feature-flagged, it should not get in the way too much, so we can actually
   have it compiled in most of the time.
 - gRPC trace propagation (cl/10532 + @simon)
   We need to wire trace propagation into our gRPC clients, so if we collect
   traces both for the client and server they will be connected.
 - Fix OTLP sending batches on shutdown.
   It seems for short-lived CLI invocations we don't end up receiving all spans.
   Ensure we flush these on ctrl-c, and regular process termination.
   See https://github.com/open-telemetry/opentelemetry-rust/issues/1395#issuecomment-2045567608
   for some context.

Later:
 - Trace propagation for HTTP clients too, using
   https://www.w3.org/TR/trace-context/ or https://www.w3.org/TR/baggage/,
   whichever makes more sense.
   Candidates: nix+http(s) protocol, object_store crates.
 - (`tracing-tracy` (cl/10952))
