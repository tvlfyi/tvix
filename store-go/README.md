# store-go

This directory contains generated golang bindings, both for the tvix-store data
models, as well as the gRPC bindings.

They are generated with `mg run //tvix:store-go:regenerate`.
These files end with `.pb.go`, and are ensured to be up to date by Ci check.

Additionally, code useful when interacting with these data structures
(ending just with `.go`) is provided.
