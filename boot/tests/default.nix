{ depot, pkgs, ... }:

depot.nix.readTree.drvTargets {
  # Seed a tvix-store with the tvix docs, then start a VM, ask it to list all
  # files in /nix/store, and ensure the store path is present, which acts as a
  # nice smoketest.
  docs = pkgs.stdenv.mkDerivation {
    name = "run-vm";
    nativeBuildInputs = [
      depot.tvix.store
      depot.tvix.boot.runVM
    ];
    buildCommand = ''
      touch $out

      # Configure tvix to put data in the local working directory
      export BLOB_SERVICE_ADDR=sled://$PWD/blobs.sled
      export DIRECTORY_SERVICE_ADDR=sled://$PWD/directories.sled
      export PATH_INFO_SERVICE_ADDR=sled://$PWD/pathinfo.sled

      # Seed the tvix store with some data
      # Create a `docs` directory with the contents from ../docs
      # Make sure it still is called "docs" when calling import, so we can
      # predict the store path.
      cp -R ${../../docs} docs
      outpath=$(tvix-store import docs)

      echo "Store contents imported to $outpath"

      CH_CMDLINE="tvix.find" run-tvix-vm 2>&1 | tee output.txt
      grep ${../../docs} output.txt
    '';
    requiredSystemFeatures = [ "kvm" ];
  };
}
