# Verified streaming

`//tvix/castore` is a content-addressed storage system, using [blake3] as hash
function.

This means returned data is fetched by using the digest as lookup key, and can
be verified to be correct by feeding the received data through the hash function
and ensuring it matches the digest initially used for the lookup.

This means, data can be downloaded by any untrusted third-party as well, as the
received data is validated to match the digest it was originally requested with.

However, for larger blobs of data, having to download the entire blob to be able
to determine whether it's correct before being able to return it to an upper
layer takes a lot of time, and is wasteful, if we're only interested in a small
portion of it.

Especially when substituting from an untrusted third-party, we want to be able
to detect quickly if that third-party is sending us wrong data, and terminate
the connection early.

## Chunking

This problem has historically been solved by exchanging a list of smaller
chunks, which can be fetched individually.

BitTorrent for example breaks files up into smaller chunks, and maintains a list
of sha1 digests for each of these chunks. After the list has been fetched, this
allows fetching smaller parts of data selectively from untrusted third-parties.

Similarly, IPFS uses its IPLD model to content-address a Merkle DAG of chunk
nodes.

While these approaches solve the problem of being able to fetch smaller chunks,
they have a big disadvantage: the chunking parameters, and the topology of
the graph structure itself "bleed" into the hash of the entire data structure
itself.

This comes with some disadvantages:

Depending on the chunking parameters used, there's different representations for
the same data, causing less data sharing/reuse in the overall content- addressed
system, both when downloading data from third-parties, as well as benefiting
from data already available locally.

This can be workarounded by agreeing on only single way of chunking, but it's
not pretty.

## Chunking in tvix-castore

tvix-castore uses BLAKE3 as a digest function, which internally uses a fixed
chunksize of 1024 bytes.

BLAKE3 is a tree hash where all left nodes fully populated, contrary to
conventional serial hash functions. To be able to validate the hash of a node,
one only needs the hash of the (2) children, if any.

This means one only needs to the root digest to validate a construction, and
lower levels of the tree can be omitted.

This relieves us from the need of having to communicate more granular chunking
upfront, and making it part of our data model.

## Logical vs. physical chunking

Due to the properties of the BLAKE3 hash function, we have logical blocks of
1KiB, but this doesn't necessarily imply we need to restrict ourselves to these
chunk sizes.

The only thing we need to be able to read and verify an arbitrary byte range is
having the covering range of aligned 1K blocks.

## Actual implementation

 -> BlobService.Read() gets the capability to read chunks as well
 -> BlobService.Stat() can reply with a list of chunks.
      rq params: send_bao bool
         server should be able to offer bao all the way down to 1k
         some open questions w.r.t sending the whole bao until there, or just
         all the hashes on the "most granular" level
         -> we can recreate everything above up to the root hash.
         -> can we maybe add this to n0-computer/bao-tree as another outboard format?
      resp:
        - bao_shift: how many levels on the bottom were skipped.
          0 means send all the leaf node hashes (1K block size)
        - "our bao": blake3 digests for a given static chunk size + path down to the last leaf node and its data (length proof)
        - list of (b3digest,size) of all physical chunks.
          The server can do some CDC on ingestion, and communicate these chunks here.
          Chunk sizes should be a "reasonable size", TBD, probably something between 512K-4M

Depending on the bao depth received from the server, we end up with a logical
size of chunks that can be fetched in an authenticated fashion.

Assuming the bao chunk size received is 4(*1KiB bytes) (`bao_shift` of 2), and a
total blob size of 14 (*1KiB bytes), we can fetch
`[0..=3]`, `[4..=7]`, `[8..=11]` and `[12..=13]` in an authenticated fashion:

`[ 0 1 2 3 ] [ 4 5 6 7 ] [ 8 9 10 11 ] [ 12 13 ]`

Assuming the server now informs us about the following physical chunking:

`[ 0 1 ] [ 2 3 4 5 ] [ 6 ] [ 7 8 ] [ 9 10 11 12 13 14 15 ]`

To read from 0 until 4 (inclusive), we need to fetch physical chunks
`[ 0 1 ]`, `[ 2 3 4 5 ]` and `[ 6 ] [ 7 8 ]`.

`[ 0 1 ]` and `[ 2 3 4 5 ]` are obvious, they contain the data we're
interested in.

We however also need to fetch the physical chunks `[ 6 ]` and `[ 7 8 ]`, so we
can assemble `[ 4 5 6 7 ]` to verify that logical chunk.

Each physical chunk fetched can be validated to have the blake3 digest that was
communicated upfront, and can be stored in a client-side cache/storage.

If it's not there, the client can use the `BlobService.Read()` interface with
the *physical chunk digest*.

---

[blake3]: https://github.com/BLAKE3-team/BLAKE3