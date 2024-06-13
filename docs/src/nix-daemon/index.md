# Nix Daemon Protocol

The Nix Daemon protocol is what's used to communicate with the `nix-daemon`,
either on the local system (in which case the communication happens via a Unix
domain socket), or with a remote Nix (in which this is tunneled over SSH).

It uses a custom binary format which isn't too documented. The subpages here
collect serve as an in-depth detail about some of the inner workings, data types
etc.

A first implementation of this exists in
[griff/Nix.rs](https://github.com/griff/Nix.rs/tree/main).

Work is underway to port / factor this out into reusable building blocks into
the [nix-compat] crate.
