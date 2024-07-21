# Builder Protocol

The builder protocol is used by tvix-glue to trigger builds.

One goal of the protocol is to not be too tied to the Nix implementation itself,
allowing it to be used for other builds/workloads in the future.

This means the builder protocol is versatile enough to express the environment a
Nix build expects, while not being aware of "what any of this means".

For example, it is not aware of how certain environment variables are set in a
nix build, but allows specifying environent variables that should be set.

It's also not aware of what nix store paths are. Instead, it allows:

 - specifying a list of paths expected to be produced during the build
 - specifying a list of castore root nodes to be present in a specified
   `inputs_dir`.
 - specifying which paths are write-able during build.

In case all specified paths are produced, and the command specified in
`command_args` succeeds, the build is considered to be successful.

This happens to be sufficient to *also* express how Nix builds works.

Check `build/protos/build.proto` for a detailed description of the individual
fields, and the tests in `glue/src/tvix_build.rs` for some examples.

The following sections describe some aspects of Nix builds, and how this is
(planned to be) implemented with the Tvix Build protocol.

## Reference scanning
At the end of a build, Nix does scan a store path for references to other store
paths (*out of the set of all store paths present during the build*).
It does do this by (only) looking for a list of nixbase32-encoded hashes in
filenames (?), symlink targets and blob contents.

While we could do this entirely outside the builder protocol, it'd mean a build
client would be required to download the produced outputs locally, and do the
refscan there. This is undesireable, as the builder already has all produced
outputs locally, and it'd make more sense for it do do it.

Instead, we want to describe reference scanning in a generic fashion.

One proposed way to do this is to add an additional field `refscan_needles` to
the `BuildRequest` message.
If this is an non-empty list, all paths in `outputs` are scanned for these.

The `Build` response message would then be extended with an `outputs_needles`
field, containing the same number of elements as the existing `outputs` field.
In there, we'd have a list of numbers, indexing into `refscan_needles`
originally specified.

For Nix, `refscan_needles` would be populated with the nixbase32 hash parts of
every input store path and output store path. The latter is necessary to scan
for references between multi-output derivations.

This is sufficient to construct the referred store paths in each build output on
the build client.
