Tvix
====

For more information about Tvix, feel free to reach out.
We are interested in people who would like to help us review designs,
brainstorm and describe requirements that we may not yet have considered.

Most of the discussion around development happens on our IRC channel, which
you can join in several ways documented on
[tvl.fyi](https://tvl.fyi/#getting-in-touch).

There's also some discussion around development on our
[mailing list](https://inbox.tvl.su).

## Building the CLI

The CLI can also be built with standard Rust tooling (i.e. `cargo build`),
as long as you are in a shell with the right dependencies.

 - If you cloned the full monorepo, it can be provided by `mg shell //
   tvix:shell`.
 - If you cloned the `tvix` workspace only
   (`git clone https://code.tvl.fyi/depot.git:workspace=views/tvix.git`),
   `nix-shell` provides it.

If you're in the TVL monorepo, you can also run `mg build //tvix/cli`
(or `mg build` from inside that folder) for a more incremental build.

Please follow the depot-wide instructions on how to get `mg` and use the depot
tooling.

### Compatibility
**Important note:** We only use and test Nix builds of our software
against Nix 2.3. There are a variety of bugs and subtle problems in
newer Nix versions which we do not have the bandwidth to address,
builds in newer Nix versions may or may not work.

## Rust projects, crate2nix

Some parts of Tvix are written in Rust. To simplify the dependency
management on the Nix side of these builds, we use `crate2nix` in a
single Rust workspace in `//tvix` to maintain the Nix build
configuration.

When making changes to Cargo dependency configuration in any of the
Rust projects under `//tvix`, be sure to run
`mg run //tvix:crate2nixGenerate --` in `//tvix` itself and commit the changes
to the generated `Cargo.nix` file. This only applies to the full TVL checkout.

## License structure

All code implemented for Tvix is licensed under the GPL-3.0, with the
exception of the protocol buffer definitions used for communication
between services which are available under a more permissive license
(MIT).

The idea behind this structure is that any direct usage of our code
(e.g. linking to it, embedding the evaluator, etc.) will fall under
the terms of the GPL3, but users are free to implement their own
components speaking these protocols under the terms of the MIT
license.
