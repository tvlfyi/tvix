# BlobStore: Protocol / Composition

This documents describes the protocol that BlobStore uses to substitute blobs
other ("remote") BlobStores.

How to come up with the blake3 digest of the blob to fetch is left to another
layer in the stack.

To put this into the context of Tvix as a Nix alternative, a blob represents an
individual file inside a StorePath.
In the Tvix Data Model, this is accomplished by having a `FileNode` (either the
`root_node` in a `PathInfo` message, or a individual file inside a `Directory`
message) encode a BLAKE3 digest.

However, the whole infrastructure can be applied for other usecases requiring
exchange/storage or access into data of which the blake3 digest is known.

## Protocol and Interfaces
As an RPC protocol, BlobStore currently uses gRPC.

On the Rust side of things, every blob service implements the
[`BlobService`](../src/blobservice/mod.rs) async trait, which isn't
gRPC-specific.

This `BlobService` trait provides functionality to check for existence of Blobs,
read from blobs, and write new blobs.
It also provides a method to ask for more granular chunks if they are available.

In addition to some in-memory, on-disk and (soon) object-storage-based
implementations, we also have a `BlobService` implementation that talks to a
gRPC server, as well as a gRPC server wrapper component, which provides a gRPC
service for anything implementing the `BlobService` trait.

This makes it very easy to talk to a remote `BlobService`, which does not even
need to be written in the same language, as long it speaks the same gRPC
protocol.

It also puts very little requirements on someone implementing a new
`BlobService`, and how its internal storage or chunking algorithm looks like.

The gRPC protocol is documented in `../protos/rpc_blobstore.proto`.
Contrary to the `BlobService` trait, it does not have any options for seeking/
ranging, as it's more desirable to provide this through chunking (see also
`./blobstore-chunking.md`).

## Composition
Different `BlobStore` are supposed to be "composed"/"layered" to express
caching, multiple local and remote sources.

The fronting interface can be the same, it'd just be multiple "tiers" that can
respond to requests, depending on where the data resides. [^1]

This makes it very simple for consumers, as they don't need to be aware of the
entire substitutor config.

The flexibility of this doesn't need to be exposed to the user in the default
case; in most cases we should be fine with some form of on-disk storage and a
bunch of substituters with different priorities.

### gRPC Clients
Clients are encouraged to always read blobs in a chunked fashion (asking for a
list of chunks for a blob via `BlobService.Stat()`, then fetching chunks via
`BlobService.Read()` as needed), instead of directly reading the entire blob via
`BlobService.Read()`.

In a composition setting, this provides opportunity for caching, and avoids
downloading some chunks if they're already present locally (for example, because
they were already downloaded by reading from a similar blob earlier).

It also removes the need for seeking to be a part of the gRPC protocol
alltogether, as chunks are supposed to be "reasonably small" [^2].

There's some further optimization potential, a `BlobService.Stat()` request
could tell the server it's happy with very small blobs just being inlined in
an additional additional field in the response, which would allow clients to
populate their local chunk store in a single roundtrip.

## Verified Streaming
As already described in `./docs/blobstore-chunking.md`, the physical chunk
information sent in a `BlobService.Stat()` response is still sufficient to fetch
in an authenticated fashion.

The exact protocol and formats are still a bit in flux, but here's some notes:

 - `BlobService.Stat()` request gets a `send_bao` field (bool), signalling a
   [BAO][bao-spec] should be sent. Could also be `bao_shift` integer, signalling
   how detailed (down to the leaf chunks) it should go.
   The exact format (and request fields) still need to be defined, edef has some
   ideas around omitting some intermediate hash nodes over the wire and
   recomputing them, reducing size by another ~50% over [bao-tree].
 - `BlobService.Stat()` response gets some bao-related fields (`bao_shift`
   field, signalling the actual format/shift level the server replies with, the
   actual bao, and maybe some format specifier).
   It would be nice to also be compatible with the baos used by [iroh], so we
   can provide an implementation using it too.

---

[^1]: We might want to have some backchannel, so it becomes possible to provide
      feedback to the user that something is downloaded.
[^2]: Something between 512K-4M, TBD.
[bao-spec]: https://github.com/oconnor663/bao/blob/master/docs/spec.md
[bao-tree]: https://github.com/n0-computer/bao-tree
[iroh]: https://github.com/n0-computer/iroh
