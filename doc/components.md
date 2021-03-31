---
title: "Tvix - Architecture & data flow"
numbersections: true
author:
- adisbladis
- flokli
- tazjin
email:
- adis@blad.is
lang: en-GB
classoption:
- twocolumn
header-includes:
- \usepackage{caption, graphicx, tikz, aeguill, pdflscape}
---

# Background
We intend for Tvix tooling to be more decoupled than the existing, monolithic Nix implementation. In practice we expect to gain several benefits from this, such as:

- Ability to use different builders
- Ability to use different store implementations
- No monopolisation of the implementation, allowing users to replace components that they are unhappy with (up to and including the language evaluator)
- Less hidden intra-dependencies between tools due to explicit RPC/IPC boundaries

Communication between different components of the system will use gRPC. The rest of this document outlines the components.

# Components

## Coordinator

*Purpose:* The coordinator (in the simplest case, the Tvix CLI tool) oversees the flow of a build process and delegates tasks to the right subcomponents. For example, if a user runs the equivalent of nix-build in a folder containing a default.nix file the coordinator will invoke the evaluator, pass the resulting derivations to the builder and coordinate any necessary store interactions (for substitution and other purposes).

While many users are likely to use the CLI tool as their main method of interacting with Tvix, it is not unlikely that alternative coordinators (e.g. for a distributed, “Nix-native” CI system) would be implemented. To facilitate this, we are considering to implement the coordinator on top of a state-machine model that would make it possible to reuse the FSM logic without tying it to any particular kind of application.

## Evaluator

*Purpose:* Eval takes care of evaluating Nix code. In a typical build flow, it would be responsible for producing derivations. It can however also be used as a standalone tool, for example in use-cases where Nix is used to generate configuration without any build or store involvement.

*Requirements:* As of now, it will run on the machine invoking the build command itself. For now, we give it filesystem access, so things like imports, `builtins.readFile` etc. can be handled.

In the future, we might be able to abstract away raw filesystem access, by allowing the Evaluator to request files from the coordinator (which will query the Store for it). This might get messy, and the benefits are questionable. We might be fine with running the evaluator with filesystem access for now, and can extend the interface if the need arises.

## Builder

*Purpose:* A builder receives derivations from the coordinator and builds them.

By making builder a standardised interface it's possible to make the sandboxing mechanism used by the build process pluggable.

Nix is currently using a hard-coded [libseccomp](https://github.com/seccomp/libseccomp) based sandboxing mechanism and another one based on [sandboxd](https://www.unix.com/man-page/mojave/8/sandboxd/) on MacOS.
These are only separated by [compiler preprocessor macros](https://gcc.gnu.org/onlinedocs/cpp/Ifdef.html) within the same source files despite having very little in common with each other.

This makes experimentation with alternative backends difficult and porting Nix to other platforms harder than it has to be.
We want to switch the Linux build sandbox to use [OCI](https://github.com/opencontainers/runtime-spec), the current dominant Linux containerisation technology, by default.

With a well-defined builder abstraction it's also easy to imagine other backends such as a Kubernetes-based one in the future.

## Store

*Purpose:* Store takes care of storing build results. It provides a unified interface to get file paths, and upload new ones.

Most likely we will end up with multiple implementations of Store, a few possible ones that comes to mind are:
- Local
- SSH
- GCP
- S3
- Ceph

# Figures

![component flow](./component-flow.svg)
