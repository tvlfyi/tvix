let
  path = builtins.unsafeDiscardStringContext "${../empty-file}";
  storePath = builtins.storePath path;
  context = builtins.getContext storePath;
in
{
  hasContext = builtins.hasContext storePath;
  contextMatches = context == { "${path}" = { path = true; }; };
}
