let
  path = builtins.unsafeDiscardStringContext "${../dummy}";
  storePath = builtins.storePath path;
in
{
  plain = builtins.storePath path;
  withSubPath = builtins.storePath (path + "/.keep");
}
