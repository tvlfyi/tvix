tvix-[ca]store API
==============

This document outlines the design of the API exposed by tvix-castore and tvix-
store, as well as other implementations of this store protocol.

This document is meant to be read side-by-side with
[castore.md](../../tvix-castore/docs/castore.md) which describes the data model
in more detail.

The store API has four main consumers:

1. The evaluator (or more correctly, the CLI/coordinator, in the Tvix
   case) communicates with the store to:

   * Upload files and directories (e.g. from `builtins.path`, or `src = ./path`
     Nix expressions).
   * Read files from the store where necessary (e.g. when `nixpkgs` is
     located in the store, or for IFD).

2. The builder communicates with the store to:

   * Upload files and directories after a build, to persist build artifacts in
     the store.

3. Tvix clients (such as users that have Tvix installed, or, depending
   on perspective, builder environments) expect the store to
   "materialise" on disk to provide a directory layout with store
   paths.

4. Stores may communicate with other stores, to substitute already built store
   paths, i.e. a store acts as a binary cache for other stores.

The store API attempts to reuse parts of its API between these three
consumers by making similarities explicit in the protocol. This leads
to a protocol that is slightly more complex than a simple "file
upload/download"-system, but at significantly greater efficiency, both in terms
of deduplication opportunities as well as granularity.

## The Store model

Contents inside a tvix-store can be grouped into three different message types:

 * Blobs
 * Directories
 * PathInfo (see further down)

(check `castore.md` for more detailed field descriptions)

### Blobs
A blob object contains the literal file contents of regular (or executable)
files.

### Directory
A directory object describes the direct children of a directory.

It contains:
 - name of child (regular or executable) files, and their [blake3][blake3] hash.
 - name of child symlinks, and their target (as string)
 - name of child directories, and their [blake3][blake3] hash (forming a Merkle DAG)

### Content-addressed Store Model
For example, lets consider a directory layout like this, with some
imaginary hashes of file contents:

```
.
├── file-1.txt        hash: 5891b5b522d5df086d0ff0b110fb
└── nested
    └── file-2.txt    hash: abc6fd595fc079d3114d4b71a4d8
```

A hash for the *directory* `nested` can be created by creating the `Directory`
object:

```json
{
  "directories": [],
  "files": [{
    "name": "file-2.txt",
    "digest": "abc6fd595fc079d3114d4b71a4d8",
    "size": 123,
  }],
  "symlink": [],
}
```

And then hashing a serialised form of that data structure. We use the blake3
hash of the canonical protobuf representation. Let's assume the hash was
`ff0029485729bcde993720749232`.

To create the directory object one layer up, we now refer to our `nested`
directory object in `directories`, and to `file-1.txt` in `files`:

```json
{
  "directories": [{
    "name": "nested",
    "digest": "ff0029485729bcde993720749232",
    "size": 1,
  }],
  "files": [{
    "name": "file-1.txt",
    "digest": "5891b5b522d5df086d0ff0b110fb",
    "size": 124,
  }]
}
```

This Merkle DAG of Directory objects, and flat store of blobs can be used to
describe any file/directory/symlink inside a store path. Due to its content-
addressed nature, it'll automatically deduplicate (re-)used (sub)directories,
and allow substitution from any (untrusted) source.

The thing that's now only missing is the metadata to map/"mount" from the
content-addressed world to a physical path.

### PathInfo
As most paths in the Nix store currently are input-addressed [^input-addressed],
and the `tvix-castore` data model is also not intrinsically using NAR hashes,
we need something mapping from an input-addressed "output path hash" (or a Nix-
specific content-addressed path) to the contents in the `tvix-castore` world.

That's what `PathInfo` provides. It embeds the root node (Directory, File or
Symlink) at a given store path.

The root nodes' `name` field is populated with the (base)name inside
`/nix/store`, so `xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx-pname-1.2.3`.

The `PathInfo` message also stores references to other store paths, and some
more NARInfo-specific metadata (signatures, narhash, narsize).


## API overview

There's three different services:

### BlobService
`BlobService` can be used to store and retrieve blobs of data, used to host
regular file contents.

It is content-addressed, using [blake3][blake3]
as a hashing function.

As blake3 is a tree hash, there's an opportunity to do
[verified streaming][bao] of parts of the file,
which doesn't need to trust any more information than the root hash itself.
Future extensions of the `BlobService` protocol will enable this.

### DirectoryService
`DirectoryService` allows lookups (and uploads) of `Directory` messages, and
whole reference graphs of them.


### PathInfoService
The PathInfo service provides lookups from a store path hash to a `PathInfo`
message.

## Example flows

Below there are some common use cases of tvix-store, and how the different
services are used.

###  Upload files and directories
This is needed for `builtins.path` or `src = ./path` in Nix expressions (A), as
well as for uploading build artifacts to a store (B).

The path specified needs to be (recursively, BFS-style) traversed.
 * All file contents need to be hashed with blake3, and submitted to the
   *BlobService* if not already present.
   A reference to them needs to be added to the parent Directory object that's
   constructed.
 * All symlinks need to be added to the parent directory they reside in.
 * Whenever a Directory has been fully traversed, it needs to be uploaded to
   the *DirectoryService* and a reference to it needs to be added to the parent
   Directory object.

Most of the hashing / directory traversal/uploading can happen in parallel,
as long as Directory objects only refer to Directory objects and Blobs that
have already been uploaded.

When reaching the root, a `PathInfo` object needs to be constructed.

 * In the case of content-addressed paths (A), the name of the root node is
   based on the NAR representation of the contents.
   It might make sense to be able to offload the NAR calculation to the store,
   which can cache it.
 * In the case of build artifacts (B), the output path is input-addressed and
   known upfront.

Contrary to Nix, this has the advantage of not having to upload a lot of things
to the store that didn't change.

### Reading files from the store from the evaluator
This is the case when `nixpkgs` is located in the store, or IFD in general.

The store client asks the `PathInfoService` for the `PathInfo` of the output
path in the request, and looks at the root node.

If something other than the root of the store path is requested, like for
example `maintainers/maintainer-list.nix`, the root_node Directory is inspected
and potentially a chain of `Directory` objects requested from
*DirectoryService*. [^n+1query].

When the desired file is reached, the *BlobService* can be used to read the
contents of this file, and return it back to the evaluator.

FUTUREWORK: define how importing from symlinks should/does work.

Contrary to Nix, this has the advantage of not having to copy all of the
contents of a store path to the evaluating machine, but really only fetching
the files the evaluator currently cares about.

### Materializing store paths on disk
This is useful for people running a Tvix-only system, or running builds on a
"Tvix remote builder" in its own mount namespace.

In a system with Nix installed, we can't simply manually "extract" things to
`/nix/store`, as Nix assumes to own all writes to this location.
In these use cases, we're probably better off exposing a tvix-store as a local
binary cache (that's what `//tvix/nar-bridge-go` does).

Assuming we are in an environment where we control `/nix/store` exclusively, a
"realize to disk" would either "extract" things from the `tvix-store` to a
filesystem, or expose a `FUSE`/`virtio-fs` filesystem.

The latter is already implemented, and particularly interesting for (remote)
build workloads, as build inputs can be realized on-demand, which saves copying
around a lot of never- accessed files.

In both cases, the API interactions are similar.
 * The *PathInfoService* is asked for the `PathInfo` of the requested store path.
 * If everything should be "extracted", the *DirectoryService* is asked for all
   `Directory` objects in the closure, the file structure is created, all Blobs
   are downloaded and placed in their corresponding location and all symlinks
   are created accordingly.
 * If this is a FUSE filesystem, we can decide to only request a subset,
   similar to the "Reading files from the store from the evaluator" use case,
   even though it might make sense to keep all Directory objects around.
   (See the caveat in "Trust model" though!)

### Stores communicating with other stores
The gRPC API exposed by the tvix-store allows composing multiple stores, and
implementing some caching strategies, that store clients don't need to be aware
of.

 * For example, a caching strategy could have a fast local tvix-store, that's
   asked first and filled with data from a slower remote tvix-store.

 * Multiple stores could be asked for the same data, and whatever store returns
   the right data first wins.


## Trust model / Distribution
As already described above, the only non-content-addressed service is the
`PathInfo` service.

This means, all other messages (such as `Blob` and `Directory` messages) can be
substituted from many different, untrusted sources/mirrors, which will make
plugging in additional substitution strategies like IPFS, local network
neighbors super simple. That's also why it's living in the `tvix-castore` crate.

As for `PathInfo`, we don't specify an additional signature mechanism yet, but
carry the NAR-based signatures from Nix along.

This means, if we don't trust a remote `PathInfo` object, we currently need to
"stream" the NAR representation to validate these signatures.

However, the slow part is downloading of NAR files, and considering we have
more granularity available, we might only need to download some small blobs,
rather than a whole NAR file.

A future signature mechanism, that is only signing (parts of) the `PathInfo`
message, which only points to content-addressed data will enable verified
partial access into a store path, opening up opportunities for lazy filesystem
access etc.



[blake3]: https://github.com/BLAKE3-team/BLAKE3
[bao]: https://github.com/oconnor663/bao
[^input-addressed]: Nix hashes the A-Term representation of a .drv, after doing
                    some replacements on refered Input Derivations to calculate
                    output paths.
[^n+1query]: This would expose an N+1 query problem. However it's not a problem
             in practice, as there's usually always a "local" caching store in
             the loop, and *DirectoryService* supports a recursive lookup for
             all `Directory` children of a `Directory`
