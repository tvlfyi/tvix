[
  # builtins.toXML retains context where there is.
  (builtins.getContext (builtins.toXML {
    inherit (derivation {
      name = "test";
      builder = "/bin/sh";
      system = builtins.currentSystem;
    }) drvPath;
  }))

  # this should have no context.
  (builtins.hasContext
    (builtins.toXML { }))
]
