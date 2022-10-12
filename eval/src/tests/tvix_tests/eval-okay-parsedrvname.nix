
# the first dash followed by a non-alphabetic character separates
# the "name" from the "version"

assert builtins.parseDrvName "ripgrep-1.2"       == { name = "ripgrep";       version = "1.2"; };
assert builtins.parseDrvName "rip-grep-1.2"      == { name = "rip-grep";      version = "1.2"; };
assert builtins.parseDrvName "7zip_archiver-0.2" == { name = "7zip_archiver"; version = "0.2"; };
assert builtins.parseDrvName "gcc-1-2"           == { name = "gcc";           version = "1-2"; };
assert builtins.parseDrvName "bash--1-2"         == { name = "bash";          version = "-1-2"; };
assert builtins.parseDrvName "xvidtune-?1-2"     == { name = "xvidtune";      version = "?1-2"; };

true
