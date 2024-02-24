<img class="tvl-logo" src="./logo.webp"
     alt="A candy bar in different shades of blue that says 'Tvix by TVL' on it">

------------------

Tvix is a new implementation of Nix, a purely-functional package manager. It
aims to have a modular implementation, in which different components can be
reused or replaced based on the use-case.

Tvix is developed as a GPLv3-licensed open-source project by
[TVL][], with source code available in the [TVL monorepo][].

There are several projects within Tvix, such as:

* `//tvix/castore` - subtree storage/transfer in a content-addressed fashion
* `//tvix/cli` - preliminary REPL & CLI implementation for Tvix
* `//tvix/eval` - an implementation of the Nix programming language
* `//tvix/nar-bridge` - a HTTP webserver providing a Nix HTTP Binary Cache interface in front of a tvix-store
* `//tvix/nix-compat` - a Rust library for compatibility with C++ Nix, features like encodings and hashing schemes and formats
* `//tvix/serde` - a Rust library for using the Nix language for app configuration
* `//tvix/store` - a "filesystem" linking Nix store paths and metadata with the content-addressed layer
* ... and a handful others!

The language evaluator can be toyed with in [Tvixbolt][], and you can check out
the [Tvix README][] ([GitHub mirror][gh]) for additional information on the
project and development workflows.

Developer documentation for some parts of Tvix is [available online][docs].

Benchmarks are run nightly on new commits by [windtunnel][wt].

[TVL]: https://tvl.fyi
[TVL monorepo]: https://cs.tvl.fyi/depot/-/tree/tvix
[Tvixbolt]: https://bolt.tvix.dev
[Tvix README]: https://code.tvl.fyi/about/tvix
[gh]: https://github.com/tvlfyi/tvix/
[docs]: https://docs.tvix.dev
[wt]: https://staging.windtunnel.ci/tvl/tvix

-------------------

Check out the latest Tvix-related blog posts from TVL's website:
