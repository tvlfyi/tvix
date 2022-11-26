# //tvix/store/docs/castore.md

This provides some more notes on the fields used in castore.proto.

It's meant to supplement `//tvix/store/docs/api.md`.

## Directory message
`Directory` messages use the blake3 hash of their canonical protobuf
serialization as its identifier.

A `Directory` message contains three lists, `directories`, `files` and
`symlinks`, holding `DirectoryNode`, `FileNode` and `SymlinkNode` messages
respectively. They describe all the direct child elements that are contained in
a directory.

All three message types have a `name` field, specifying the (base)name of the
element (which MUST not contain slashes or null bytes, and MUST not be '.' or '..').
For reproducibility reasons, the lists MUST be sorted by that name and also
MUST be unique across all three lists.

In addition to the `name` field, the various *Node messages have the following
fields:

## DirectoryNode
A `DirectoryNode` message represents a child directory.

It has a `digest` field, which points to the identifier of another `Directory`
message, making a `Directory` a merkle tree (or strictly speaking, a graph, as
two elements pointing to a child directory with the same contents would point
to the same `Directory` message.

There's also a `size` field, containing the (total) number of all child
elements in the referenced `Directory`, which helps for inode calculation.

## FileNode
A `FileNode` message represents a child (regular) file.

Its `digest` field contains the blake3 hash of the file contents. It can be
looked up in the `BlobService`.

The `size` field contains the size of the blob the `digest` field refers to.

The `executable` field specifies whether the file should be marked as
executable or not.

## SymlinkNode
A `SymlinkNode` message represents a child symlink.

In addition to the `name` field, the only additional field is the `target`,
which is a string containing the target of the symlink.
