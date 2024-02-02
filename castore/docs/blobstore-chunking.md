# BlobStore: Chunking & Verified Streaming

`tvix-castore`'s BlobStore is a content-addressed storage system, using [blake3]
as hash function.

Returned data is fetched by using the digest as lookup key, and can be verified
to be correct by feeding the received data through the hash function and
ensuring it matches the digest initially used for the lookup.

This means, data can be downloaded by any untrusted third-party as well, as the
received data is validated to match the digest it was originally requested with.

However, for larger blobs of data, having to download the entire blob at once is
wasteful, if we only care about a part of the blob. Think about mounting a
seekable data structure, like loop-mounting an .iso file, or doing partial reads
in a large Parquet file, a column-oriented data format.

> We want to have the possibility to *seek* into a larger file.

This however shouldn't compromise on data integrity properties - we should not
need to trust a peer we're downloading from to be "honest" about the partial
data we're reading. We should be able to verify smaller reads.

Especially when substituting from an untrusted third-party, we want to be able
to detect quickly if that third-party is sending us wrong data, and terminate
the connection early.

## Chunking
In content-addressed systems, this problem has historically been solved by
breaking larger blobs into smaller chunks, which can be fetched individually,
and making a hash of *this listing* the blob digest/identifier.

 - BitTorrent for example breaks files up into smaller chunks, and maintains
   a list of sha1 digests for each of these chunks. Magnet links contain a
   digest over this listing as an identifier. (See [bittorrent-v2][here for
   more details]).
   With the identifier, a client can fetch the entire list, and then recursively
   "unpack the graph" of nodes, until it ends up with a list of individual small
   chunks, which can be fetched individually.
 - Similarly, IPFS with its IPLD model builds up a Merkle DAG, and uses the
   digest of the root node as an identitier.

These approaches solve the problem of being able to fetch smaller chunks in a
trusted fashion. They can also do some deduplication, in case there's the same
leaf nodes same leaf nodes in multiple places.

However, they also have a big disadvantage. The chunking parameters, and the
"topology" of the graph structure itself "bleed" into the root hash of the
entire data structure itself.

Depending on the chunking parameters used, there's different representations for
the same data, causing less data sharing/reuse in the overall system, in terms of how
many chunks need to be downloaded vs. are already available locally, as well as
how compact data is stored on-disk.

This can be workarounded by agreeing on only a single way of chunking, but it's
not pretty and misses a lot of deduplication potential.

### Chunking in Tvix' Blobstore
tvix-castore's BlobStore uses a hybrid approach to eliminate some of the
disadvantages, while still being content-addressed internally, with the
highlighted benefits.

It uses [blake3] as hash function, and the blake3 digest of **the raw data
itself** as an identifier (rather than some application-specific Merkle DAG that
also embeds some chunking information).

BLAKE3 is a tree hash where all left nodes fully populated, contrary to
conventional serial hash functions. To be able to validate the hash of a node,
one only needs the hash of the (2) children [^1], if any.

This means one only needs to the root digest to validate a constructions, and these
constructions can be sent [separately][bao-spec].

This relieves us from the need of having to encode more granular chunking into
our data model / identifier upfront, but can make this a mostly a transport/
storage concern.

For the some more description on the (remote) protocol, check
`./blobstore-protocol.md`.

#### Logical vs. physical chunking

Due to the properties of the BLAKE3 hash function, we have logical blocks of
1KiB, but this doesn't necessarily imply we need to restrict ourselves to these
chunk sizes w.r.t. what "physical chunks" are sent over the wire between peers,
or are stored on-disk.

The only thing we need to be able to read and verify an arbitrary byte range is
having the covering range of aligned 1K blocks, and a construction from the root
digest to the 1K block.

Note the intermediate hash tree can be further trimmed, [omitting][bao-tree]
lower parts of the tree while still providing verified streaming - at the cost
of having to fetch larger covering ranges of aligned blocks.

Let's pick an example. We identify each KiB by a number here for illustrational
purposes.

Assuming we omit the last two layers of the hash tree, we end up with logical
4KiB leaf chunks (`bao_shift` of `2`).

For a blob of 14 KiB total size, we could fetch logical blocks `[0..=3]`,
`[4..=7]`, `[8..=11]` and `[12..=13]` in an authenticated fashion:

`[ 0 1 2 3 ] [ 4 5 6 7 ] [ 8 9 10 11 ] [ 12 13 ]`

Assuming the server now informs us about the following physical chunking:

```
[ 0 1 ] [ 2 3 4 5 ] [ 6 ] [ 7 8 ] [ 9 10 11 12 13 14 15 ]`
```

If our application now wants to arbitrarily read from 0 until 4 (inclusive):

```
[ 0 1 ] [ 2 3 4 5 ] [ 6 ] [ 7 8 ] [ 9 10 11 12 13 14 15 ]
 |-------------|

```

…we need to fetch physical chunks `[ 0 1 ]`, `[ 2 3 4 5 ]` and `[ 6 ] [ 7 8 ]`.


`[ 0 1 ]` and `[ 2 3 4 5 ]` are obvious, they contain the data we're
interested in.

We however also need to fetch the physical chunks `[ 6 ]` and `[ 7 8 ]`, so we
can assemble `[ 4 5 6 7 ]` to verify both logical chunks:

```
[ 0 1 ] [ 2 3 4 5 ] [ 6 ] [ 7 8 ] [ 9 10 11 12 13 14 15 ]
^       ^           ^     ^
|----4KiB----|------4KiB-----|
```

Each physical chunk fetched can be validated to have the blake3 digest that was
communicated upfront, and can be stored in a client-side cache/storage, so
subsequent / other requests for the same data will be fast(er).

---

[^1]: and the surrounding context, aka position inside the whole blob, which is available while verifying the tree
[bittorrent-v2]: https://blog.libtorrent.org/2020/09/bittorrent-v2/
[blake3]: https://github.com/BLAKE3-team/BLAKE3
[bao-spec]: https://github.com/oconnor663/bao/blob/master/docs/spec.md
[bao-tree]: https://github.com/n0-computer/bao-tree
