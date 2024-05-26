<div align="center">
  <img src="https://tvix.dev/logo.webp">
</div>

-----------------

Tvix is a new implementation of the Nix language and package manager. See the
[announcement post][post-1] for information about the background of this
project.

Tvix is developed by [TVL][tvl] in our monorepo, the `depot`, at
[//tvix][tvix-src]. Code reviews take place on [Gerrit][tvix-gerrit], bugs are
filed in [our issue tracker][b].

For more information about Tvix, feel free to reach out. We are interested in
people who would like to help us review designs, brainstorm and describe
requirements that we may not yet have considered.

Most of the discussion around development happens in our dedicated IRC channel,
[`#tvix-dev`][tvix-dev-irc] on [hackint][],
which is also reachable [via XMPP][hackint-xmpp]
at [`#tvix-dev@irc.hackint.org`][tvix-dev-xmpp] (sic!)
and [via Matrix][hackint-matrix] at [`#tvix-dev:hackint.org`][tvix-dev-matrix].

There's also the IRC channel of the [wider TVL community][tvl-getting-in-touch],
less on-topic, or our [mailing list][].

Contributions to Tvix follow the TVL [review flow][review-docs] and
[contribution guidelines][contributing].

[post-1]: https://tvl.fyi/blog/rewriting-nix
[tvl]: https://tvl.fyi
[tvix-src]: https://cs.tvl.fyi/depot/-/tree/tvix/
[tvix-gerrit]: https://cl.tvl.fyi/q/path:%255Etvix.*
[b]: https://b.tvl.fyi
[tvl-getting-in-touch]: https://tvl.fyi/#getting-in-touch
[mailing list]: https://inbox.tvl.su
[review-docs]: https://code.tvl.fyi/about/docs/REVIEWS.md
[contributing]: https://code.tvl.fyi/about/docs/CONTRIBUTING.md
[tvix-dev-irc]: ircs://irc.hackint.org:6697/#tvix-dev
[hackint]: https://hackint.org/
[hackint-xmpp]: https://hackint.org/transport/xmpp
[tvix-dev-xmpp]: xmpp:#tvix-dev@irc.hackint.org?join
[hackint-matrix]: https://hackint.org/transport/matrix
[tvix-dev-matrix]: https://matrix.to/#/#tvix-dev:hackint.org
[tvix-dev-webchat]: https://webirc.hackint.org/#ircs://irc.hackint.org/#tvix-dev

WARNING: Tvix is not ready for use in production. None of our current APIs
should be considered stable in any way.

WARNING: Any other instances of this project or repository are
[`josh`-mirrors][josh]. We do not accept code contributions or issues outside of
the tooling and communication methods outlined above.

[josh]: https://github.com/josh-project/josh

## Components

This folder contains the following components:

* `//tvix/castore` - subtree storage/transfer in a content-addressed fashion
* `//tvix/cli` - preliminary REPL & CLI implementation for Tvix
* `//tvix/eval` - an implementation of the Nix programming language
* `//tvix/nar-bridge-go` - a HTTP webserver providing a Nix HTTP Binary Cache interface in front of a tvix-store
* `//tvix/nix-compat` - a Rust library for compatibility with C++ Nix, features like encodings and hashing schemes and formats
* `//tvix/serde` - a Rust library for using the Nix language for app configuration
* `//tvix/store` - a "filesystem" linking Nix store paths and metadata with the content-addressed layer

Some additional folders with auxiliary things exist and can be explored at your
leisure.

## Building the CLI

The CLI can also be built with standard Rust tooling (i.e. `cargo build`),
as long as you are in a shell with the right dependencies.

 - If you cloned the full monorepo, it can be provided by
   `mg shell //tvix:shell`.
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
`mg run //tools:crate2nix-generate` in `//tvix` itself and commit the changes
to the generated `Cargo.nix` file. This only applies to the full TVL checkout.

When adding/removing a Cargo feature for a crate, you will want to add it to the
features power set that gets tested in CI. For each crate there's a default.nix with a
`mkFeaturePowerset` invocation, modify the list to include/remove the feature.
Note that you don't want to add "collection" features, such as `fs` for tvix-[ca]store or `default`.

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
