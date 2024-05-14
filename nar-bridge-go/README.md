# //tvix/nar-bridge-go

This exposes a HTTP Binary cache interface (GET/HEAD/PUT requests) for a `tvix-
store`.

It can be used to configure a tvix-store as a substitutor for Nix, or to upload
store paths from Nix via `nix copy` into a `tvix-store`.
