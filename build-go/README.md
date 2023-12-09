# build-go

This directory contains generated golang bindings, both for the `tvix-build`
data models, as well as the gRPC bindings.

They are generated with `mg run //tvix/build-go:regenerate`.
These files end with `.pb.go`, and are ensured to be up to date by a CI check.

Additionally, code useful when interacting with these data structures
(ending just with `.go`) is provided.
