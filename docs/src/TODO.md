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

### castore
  - The `Directory` struct is currently being generated from the protobuf definitions.
    We should change this, have `Directory` be our own type (holding a `B3Digest`),
    rather than a `Vec<u8>`, and move impls to there.
    The protobuf struct can stay in `castore/proto`, but should only be used
    when converting to/from Protobuf definitons:
     - gRPC client/server impls
     - implementations that use protobuf as a internal data format
     - `digest()` implementation of our stricter `Directory` struct

    The traits should use our stricter type. This would allow removing a lot of
    the checks we currently have when converting from `Vec<u8>` to `B3Digest` in
    various places.

### Correctness > Performance
A lot of the Nix behaviour isn't well documented out, and before going too deep
into performance optimizations, we need to ensure we properly grasped all hidden
features. This is to avoid doing a lot of "overall architecture perf-related
work" and increased code complexity based on a mental model that might get
disproved later on, as we work towards correctness.

We do this by evaluating more and more parts of the official Nix test suite, as
well as our own Tvix test suite, and compare it with Nix' output.

Additionally, we evaluate attributes from nixpkgs, compare calculated output
paths (to determine equivalence of evaluated A-Terms) and fix differences as we
encounter them.

This currently is a very manual and time-consuming process, both in terms of
setup, as well as spotting the source of the differences (and "compensating" for
the resulting diff noise on resulting mismtaches).

 - We could use some better tooling that periodically evaluates nixpkgs, and
   compares the output paths with the ones produced by Nix
 - We could use some better tooling that can spot the (real) differences between
   two (graphs of) derivations, while removing all resulting noise from the diff
in resulting store paths.


### Performance
Even while keeping in mind some of the above caveats, there's some obvious
low-langing fruits that could have a good impact on performance, with somewhat
limited risk of becoming obsolete in case of behaviorial changes due to
correctness:

 - String Contexts currently do a lot of indirections (edef)
   (NixString -> NixStringInner -> HashSet[element] -> NixContextElement -> String -> data)
   to get to the actual data. We should improve this. There's various ideas, one
   of it is globally interning all Nix context elements, and only keeping
   indices into that. We might need to have different representations for small
   amount of context elements or larger ones, and need tooling to reason about
   the amount of contexts we have.
 - To calculate NAR size and digest (used for output path calculation of FODs),
   our current `SimpleRenderer` `NarCalculationService` sequentially asks for
   one blob after another (and internally these might consists out of multiple
   chunks too).
   That's a lot of roundtrips, adding up to a lot of useless waiting.
   While we cannot avoid having to feed all bytes sequentially through sha256,
   we already know what blobs to fetch and in which order.
   There should be a way to buffer some "amount of upcoming bytes" in memory,
   and not requesting these seqentially.
   This is somewhat the "spiritual counterpart" to our sequential ingestion
   code (`ConcurrentBlobUploader`, used by `ingest_nar`), which keeps
   "some amount of outgoing bytes" in memory.
   This is somewhat blocked until the {Chunk/Blob}Service split is done, as then
   prefetching would only be a matter of adding it into the one `BlobReader`.

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

### Fetchers
Some more fetcher-related builtins need work:
 - `fetchGit`
 - `fetchTree` (hairy, seems there's no proper spec and the URL syntax seems
   subject to change/underdocumented)

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
   At least for the `object_store` backend, this might be a problem, causing a
   lot of round-trips. It also doesn't compose well - every implementation of
   `BlobService` needs to both solve the "holding metadata about chunking info"
   as well as "storing chunks" questions.
   Design idea (@flokli): split these two concerns into two separate traits:
    - a `ChunkService` dealing with retrieving individual chunks, by their
      content digests. Chunks are small enough to keep around in contiguous
      memory.
    - a `BlobService` storing metadata about blobs.

   Individual stores would not need to implement `BlobReader` anymore, but that
   could be a global thing with access to the whole store composition layer,
   which should make it easier to reuse chunks from other backends. Unclear
   if the write path should be structured the same way. At least for some
   backends, we want the remote end to be able to decide about chunking.

 - While `object_store` recently got support for `Content-Type`
   (https://github.com/apache/arrow-rs/pull/5650), there's no support on the
   local filesystem yet. We'd need to add support to this (through xattrs).

### PathInfoService
 - sqlite backend (different schema than the Nix one, we need the root nodes data!)

### Nix Daemon protocol
- Some work ongoing on the worker operation parsing (griff, picnoir)

### O11Y
 - Maybe drop `--log-level` entirely, and only use `RUST_LOG` env exclusively?
   `debug`,`trace` level across all crates is a bit useless, and `RUST_LOG` can
   be much more granular…
 - Trace propagation for object_store once they support a way to register a
   middleware, so we can use that to register a tracing middleware.
   https://github.com/apache/arrow-rs/issues/5990
