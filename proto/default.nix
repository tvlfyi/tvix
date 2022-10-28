# Build protocol buffer definitions to ensure that protos are valid in
# CI. Note that the output of this build target is not actually used
# anywhere, it just functions as a CI check for now.
{ pkgs, ... }:

pkgs.runCommand "tvix-cc-proto" { } ''
  mkdir $out
  ${pkgs.protobuf}/bin/protoc -I ${./.} castore.proto --cpp_out=$out
  ${pkgs.protobuf}/bin/protoc -I ${./.} evaluator.proto --cpp_out=$out
''
