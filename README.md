Tvix
====

For more information about Tvix, contact one of the project owners. We
are interested in people who would like to help us review designs,
brainstorm and describe requirements that we may not yet have
considered.

## Building the CLI

If you are in a full checkout of the TVL depot, you can simply run `mg build`
in the `cli` directory (or `mg build //tvix/cli` from anywhere in the repo).
The `mg` command is found in `/tools/magrathea`.

**Important note:** We only use and test Nix builds of our software
against Nix 2.3. There are a variety of bugs and subtle problems in
newer Nix versions which we do not have the bandwidth to address,
builds in newer Nix versions may or may not work.

The CLI can also be built with standard Rust tooling (i.e. `cargo build`),
as long as you are in a shell with the right dependencies (provided by `mg
shell //tvix:shell`).

## Rust projects, crate2nix

Some parts of Tvix are written in Rust. To simplify the dependency
management on the Nix side of these builds, we use `crate2nix` in a
single Rust workspace in `//tvix` to maintain the Nix build
configuration.

When making changes to Cargo dependency configuration in any of the
Rust projects under `//tvix`, be sure to run
`mg run //tvix:crate2nixGenerate --` in `//tvix` itself and commit the changes
to the generated `Cargo.nix` file.

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
