# nixpkgs checks against the `builtins.nixVersion` and fails if it
# doesn't like what it sees. To work around this we have a "user-agent
# style" version (see cl/6858) that ensures compatibility.

builtins.compareVersions "2.3" builtins.nixVersion
