---
title: "Tvix - Architecture & data flow"
numbersections: true
author:
- adisbladis
- flokli
- tazjin
email:
- adis@blad.is
- mail@tazj.in
lang: en-GB
classoption:
- twocolumn
header-includes:
- \usepackage{caption, graphicx, tikz, aeguill, pdflscape}
---

# Background

We intend for Tvix tooling to be more decoupled than the existing,
monolithic Nix implementation. In practice, we expect to gain several
benefits from this, such as:

- Ability to use different builders
- Ability to use different store implementations
- No monopolisation of the implementation, allowing users to replace
  components that they are unhappy with (up to and including the
  language evaluator)
- Less hidden intra-dependencies between tools due to explicit RPC/IPC
  boundaries

Communication between different components of the system will use
gRPC. The rest of this document outlines the components.

# Components

## Coordinator

*Purpose:* The coordinator (in the simplest case, the Tvix CLI tool)
oversees the flow of a build process and delegates tasks to the right
subcomponents. For example, if a user runs the equivalent of
`nix-build` in a folder containing a `default.nix` file, the
coordinator will invoke the evaluator, pass the resulting derivations
to the builder and coordinate any necessary store interactions (for
substitution and other purposes).

While many users are likely to use the CLI tool as their primary
method of interacting with Tvix, it is not unlikely that alternative
coordinators (e.g. for a distributed, "Nix-native" CI system) would be
implemented. To facilitate this, we are considering implementing the
coordinator on top of a state-machine model that would make it
possible to reuse the FSM logic without tying it to any particular
kind of application.

## Evaluator

*Purpose:* Eval takes care of evaluating Nix code. In a typical build
flow it would be responsible for producing derivations. It can also be
used as a standalone tool, for example, in use-cases where Nix is used
to generate configuration without any build or store involvement.

*Requirements:* For now, it will run on the machine invoking the build
command itself. We give it filesystem access to handle things like
imports or `builtins.readFile`.

To support IFD, the Evaluator also needs access to store paths. This
could be implemented by having the coordinator provide an interface to retrieve
files from a store path, or by ensuring a "realized version of the store" is
accessible by the evaluator (this could be a FUSE filesystem, or the "real"
/nix/store on disk.

We might be okay with running the evaluator with filesystem access for now and
can extend the interface if the need arises.

## Builder

*Purpose:* A builder receives derivations from the coordinator and
builds them.

By making builder a standardised interface it's possible to make the
sandboxing mechanism used by the build process pluggable.

Nix is currently using a hard-coded
[libseccomp](https://github.com/seccomp/libseccomp) based sandboxing
mechanism and another one based on
[sandboxd](https://www.unix.com/man-page/mojave/8/sandboxd/) on macOS.
These are only separated by [compiler preprocessor
macros](https://gcc.gnu.org/onlinedocs/cpp/Ifdef.html) within the same
source files despite having very little in common with each other.

This makes experimentation with alternative backends difficult and
porting Nix to other platforms harder than it has to be. We want to
write a new Linux builder which uses
[OCI](https://github.com/opencontainers/runtime-spec), the current
dominant Linux containerisation technology, by default.

With a well-defined builder abstraction, it's also easy to imagine
other backends such as a Kubernetes-based one in the future.

The environment in which builds happen is currently very Nix-specific. We might
want to avoid having to maintain all the intricacies of a Nix-specific
sandboxing environment in every builder, and instead only provide a more
generic interface, receiving build requests (and have the coordinator translate
derivations to that format). [^1]

To build, the builder needs to be able to mount all build inputs into the build
environment. For this, it needs the store to expose a filesystem interface.

## Store

*Purpose:* Store takes care of storing build results. It provides a
unified interface to get store paths and upload new ones, as well as querying
for the existence of a store path and its metadata (references, signatures, …).

Tvix natively uses an improved store protocol. Instead of transferring around
NAR files, which don't provide an index and don't allow seekable access, a
concept similar to git tree hashing is used.

This allows more granular substitution, chunk reusage and parallel download of
individual files, reducing bandwidth usage.
As these chunks are content-addressed, it opens up the potential for
peer-to-peer trustless substitution of most of the data, as long as we sign the
root of the index.

Tvix still keeps the old-style signatures, NAR hashes and NAR size around. In
the case of NAR hash / NAR size, this data is strictly required in some cases.
The old-style signatures are valuable for communication with existing
implementations.

Old-style binary caches (like cache.nixos.org) can still be exposed via the new
interface, by doing on-the-fly (re)chunking/ingestion.

Most likely, there will be multiple implementations of store, some storing
things locally, some exposing a "remote view".

A few possible ones that come to mind are:

- Local store
- SFTP/ GCP / S3 / HTTP
- NAR/NARInfo protocol: HTTP, S3

A remote Tvix store can be connected by simply connecting to its gRPC
interface, possibly using SSH tunneling, but there doesn't need to be an
additional "wire format" like the Nix `ssh(+ng)://` protocol.

Settling on one interface allows composition of stores, meaning it becomes
possible to express substitution from remote caches as a proxy layer.

It'd also be possible to write a FUSE implementation on top of the RPC
interface, exposing a lazily-substituting /nix/store mountpoint. Using this in
remote build context dramatically reduces the amount of data transferred to a
builder, as only the files really accessed during the build are substituted.

# Figures

![component flow](./component-flow.svg)

[^1]: There have already been some discussions in the Nix community, to switch
  to REAPI:
  https://discourse.nixos.org/t/a-proposal-for-replacing-the-nix-worker-protocol/20926/22
