# Getting Started

## Getting the code, a developer shell, & building the CLI

Tvix can be built with the Rust standard `cargo build`. A Nix shell is provided
with the correctly-versioned tooling to build.

### TVL monorepo

```console
$ git clone https://code.tvl.fyi/depot.git
$ cd depot
```

[Direnv][] is highly recommended in order to enable [`mg`][mg], a tool for
workflows in monorepos. Follow the [Direnv installation
instructions][direnv-inst], then after it’s set up continue with:

```console
$ direnv allow
$ mg shell //tvix:shell
$ cd tvix
$ cargo build
```

### Or just Tvix

At present, this option isn’t suitable for contributions & lacks the tooling of
the monorepo, but still provides a `shell.nix` which can be used for building
the Tvix project.

```console
$ git clone https://code.tvl.fyi/depot.git:workspace=views/tvix.git
$ cd tvix
$ nix-shell
$ cargo build
```


# Builds & tests

All projects are built using [Nix][] to avoid ‘build pollution’ via the user’s
local environment.

If you have Nix installed and are contributing to a project tracked in this
repository, you can usually build the project by calling `nix-build -A
path.to.project`.

For example, to build a project located at `//tools/foo` you would call
`nix-build -A tools.foo`

If the project has tests, check that they still work before submitting your
change.


[Direnv]: https://direnv.net
[direnv-inst]: https://direnv.net/docs/installation.html
[Nix]: https://nixos.org/nix/
[mg]: https://code.tvl.fyi/tree/tools/magrathea
