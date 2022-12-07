Tvix
====

For more information about Tvix, contact one of the project owners. We
are interested in people who would like to help us review designs,
brainstorm and describe requirements that we may not yet have
considered.

## Rust projects

Some parts of Tvix are written in Rust. To simplify the dependency
management on the Nix side of these builds, we use `crate2nix` in a
single Rust workspace in `//tvix` to maintain the Nix build
configuration.

When making changes to Cargo dependency configuration in any of the
Rust projects under `//tvix`, be sure to run `crate2nix generate` in
`//tvix` itself and commit the changes to the generated `Cargo.nix`
file.

`crate2nix` is available via `direnv` inside of depot, or can be built
from the `third_party.nixpkgs.crate2nix` attribute of depot. Make sure
to build it from depot to avoid generating files with a different
version that might have different output.

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
