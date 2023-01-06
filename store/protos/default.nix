# Target containing just the proto files.

{ depot, lib, ... }:

let
  inherit (lib.strings) hasSuffix;
  inherit (builtins) attrNames filter readDir;

  protoFileNames = filter (hasSuffix ".proto") (attrNames (readDir ./.));
  protoFiles = map (f: ./. + ("/" + f)) protoFileNames;
in
depot.nix.sparseTree depot.path.origSrc protoFiles
