{ depot, pkgs, ... }:

rec {
  # A binary that sets up /nix/store from virtiofs, lists all store paths, and
  # powers off the machine.
  tvix-init = depot.nix.buildGo.program {
    name = "tvix-init";
    srcs = [
      ./tvix-init.go
    ];
  };

  # A kernel with virtiofs support baked in
  kernel = pkgs.buildLinux ({ } // {
    inherit (pkgs.linuxPackages_latest.kernel) src version modDirVersion;
    autoModules = false;
    kernelPreferBuiltin = true;
    ignoreConfigErrors = true;
    kernelPatches = [ ];
    structuredExtraConfig = with pkgs.lib.kernel; {
      FUSE_FS = option yes;
      DAX_DRIVER = option yes;
      DAX = option yes;
      FS_DAX = option yes;
      VIRTIO_FS = option yes;
      VIRTIO = option yes;
      ZONE_DEVICE = option yes;
    };
  });

  # A build framework for minimal initrds
  uroot = pkgs.buildGoModule {
    pname = "u-root";
    version = "unstable-2023-09-20";
    src = pkgs.fetchFromGitHub {
      owner = "u-root";
      repo = "u-root";
      rev = "72921548ce2e88c4c5b62e83c717cbd834b58067";
      hash = "sha256-fEoUGqh6ZXprtSpJ55MeuSFe7L5A/rkIIVLCwxbPHzE=";
    };
    vendorHash = null;

    doCheck = false; # Some tests invoke /bin/bash
  };

  # Use u-root to build a initrd with our tvix-init inside.
  initrd = pkgs.stdenv.mkDerivation {
    name = "initrd.cpio";
    nativeBuildInputs = [ pkgs.go ];
    # https://github.com/u-root/u-root/issues/2466
    buildCommand = ''
      mkdir -p /tmp/go/src/github.com/u-root/
      cp -R ${uroot.src} /tmp/go/src/github.com/u-root/u-root
      cd /tmp/go/src/github.com/u-root/u-root
      chmod +w .
      cp ${tvix-init}/bin/tvix-init tvix-init

      export HOME=$(mktemp -d)
      export GOROOT="$(go env GOROOT)"

      GO111MODULE=off GOPATH=/tmp/go GOPROXY=off ${uroot}/bin/u-root -files ./tvix-init -initcmd "/tvix-init" -o $out
    '';
  };

  # Start a `tvix-store` virtiofs daemon from $PATH, then a cloud-hypervisor
  # pointed to it.
  # Supports the following env vars (and defaults)
  # CH_NUM_CPUS=1
  # CH_MEM_SIZE=512M
  # CH_CMDLINE=""
  runVM = pkgs.writers.writeBashBin "run-tvix-vm" ''
    tempdir=$(mktemp -d)

    cleanup() {
      kill $virtiofsd_pid
      if [[ -n ''${work_dir-} ]]; then
        chmod -R u+rw "$tempdir"
        rm -rf "$tempdir"
      fi
    }
    trap cleanup EXIT

    # Spin up the virtiofs daemon
    tvix-store virtiofs -l $tempdir/tvix.sock &
    virtiofsd_pid=$!

    # Wait for the socket to exist.
    until [ -e $tempdir/tvix.sock ]; do sleep 0.1; done

    CH_NUM_CPUS="''${CH_NUM_CPUS:-1}"
    CH_MEM_SIZE="''${CH_MEM_SIZE:-512M}"
    CH_CMDLINE="''${CH_CMDLINE:-}"

    # spin up cloud_hypervisor
    ${pkgs.cloud-hypervisor}/bin/cloud-hypervisor \
     --cpus boot=$CH_NUM_CPU \
     --memory mergeable=on,shared=on,size=$CH_MEM_SIZE \
     --console null \
     --serial tty \
     --kernel ${kernel.dev}/vmlinux \
     --initramfs ${initrd} \
     --cmdline "console=ttyS0 $CH_CMDLINE" \
     --fs tag=tvix,socket=$tempdir/tvix.sock,num_queues=1,queue_size=512
  '';

  # Seed a tvix-store with the tvix docs, then start a VM and search for the
  # store path in the output.
  test-docs = pkgs.stdenv.mkDerivation {
    name = "run-vm";
    nativeBuildInputs = [
      depot.tvix.store
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
      cp -R ${../docs} docs
      outpath=$(tvix-store import docs)

      echo "Store contents imported to $outpath"

      CH_CMDLINE="tvix.find" ${runVM}/bin/run-tvix-vm 2>&1 | tee output.txt
      grep ${../docs} output.txt
    '';
    requiredSystemFeatures = [ "kvm" ];
  };

  meta.ci.targets = [ "test-docs" ];
}
