# tvix/boot

This directory provides tooling to boot VMs with /nix/store provided by
virtiofs.

In the `tests/` subdirectory, there's some integration tests.

## //tvix/boot:runVM
A script spinning up a `tvix-store virtiofs` daemon, then starting a cloud-
hypervisor VM.

The cloud-hypervisor VM is using a (semi-)minimal kernel image with virtiofs
support, and a custom initrd (using u-root). It supports various command line
options, to be able to do VM tests, act as an interactive shell or exec a binary
from a closure.

It supports the following env vars:
 - `CH_NUM_CPUS=1` controls the number of CPUs available to the VM
 - `CH_MEM_SIZE=512M` controls the memory availabe to the VM
 - `CH_CMDLINE=` controls the kernel cmdline (which can be used to control the
   boot)

### Usage
First, ensure you have `tvix-store` in `$PATH`, as that's what `run-tvix-vm`
expects:

Assuming you ran `cargo build --profile=release-with-debug` before, and are in
the `tvix` directory:

```
export PATH=$PATH:$PWD/target/release-with-debug
```

Now, spin up tvix-daemon, connecting to some (local) backends:

```
tvix-store --otlp=false daemon \
  --blob-service-addr=objectstore+file://$PWD/blobs \
  --directory-service-addr=sled://$PWD/directories.sled \
  --path-info-service-addr=sled://$PWD/pathinfo.sled &
```

Copy some data into tvix-store (we use `nar-bridge` for this for now):

```
mg run //tvix:nar-bridge -- --otlp=false &
rm -Rf ~/.cache/nix; nix copy --to http://localhost:9000\?compression\=none $(mg build //third_party/nixpkgs:hello)
pkill nar-bridge
```

By default, the `tvix-store virtiofs` command used in the `runVM` script
connects to a running `tvix-store daemon` via gRPC - in which case you want to
keep `tvix-store daemon` running.

In case you want to have `tvix-store virtiofs` open the stores directly, kill
`tvix-store daemon` too, and export the addresses from above:

```
pkill tvix-store
export BLOB_SERVICE_ADDR=objectstore+file://$PWD/blobs
export DIRECTORY_SERVICE_ADDR=sled://$PWD/directories.sled
export PATH_INFO_SERVICE_ADDR=sled://$PWD/pathinfo.sled
```

#### Interactive shell
Run the VM like this:

```
CH_CMDLINE=tvix.shell mg run //tvix/boot:runVM --
```

You'll get dropped into an interactive shell, from which you can do things with
the store:

```
  ______      _         ____      _ __
 /_  __/   __(_)  __   /  _/___  (_) /_
  / / | | / / / |/_/   / // __ \/ / __/
 / /  | |/ / />  <   _/ // / / / / /_
/_/   |___/_/_/|_|  /___/_/ /_/_/\__/

/# ls -la /nix/store/
dr-xr-xr-x root 0 0   Jan  1 00:00 .
dr-xr-xr-x root 0 989 Jan  1 00:00 aw2fw9ag10wr9pf0qk4nk5sxi0q0bn56-glibc-2.37-8
dr-xr-xr-x root 0 3   Jan  1 00:00 jbwb8d8l28lg9z0xzl784wyb9vlbwss6-xgcc-12.3.0-libgcc
dr-xr-xr-x root 0 82  Jan  1 00:00 k8ivghpggjrq1n49xp8sj116i4sh8lia-libidn2-2.3.4
dr-xr-xr-x root 0 141 Jan  1 00:00 mdi7lvrn2mx7rfzv3fdq3v5yw8swiks6-hello-2.12.1
dr-xr-xr-x root 0 5   Jan  1 00:00 s2gi8pfjszy6rq3ydx0z1vwbbskw994i-libunistring-1.1
```

Once you exit the shell, the VM will power off itself.

#### Execute a specific binary
Run the VM like this:

```
hello_cmd=$(mg build //third_party/nixpkgs:hello)/bin/hello
CH_CMDLINE=tvix.run=$hello_cmd mg run //tvix/boot:runVM --
```

Observe it executing the file (and closure) from the tvix-store:

```
[    0.277486] Run /init as init process
  ______      _         ____      _ __
 /_  __/   __(_)  __   /  _/___  (_) /_
  / / | | / / / |/_/   / // __ \/ / __/
 / /  | |/ / />  <   _/ // / / / / /_
/_/   |___/_/_/|_|  /___/_/ /_/_/\__/

Hello, world!
2023/09/24 21:10:19 Nothing left to be done, powering off.
[    0.299122] ACPI: PM: Preparing to enter system sleep state S5
[    0.299422] reboot: Power down
```

#### Boot a NixOS system closure
It's also possible to boot a system closure. To do this, tvix-init honors the
init= cmdline option, and will `switch_root` to it.

Make sure to first copy that system closure into tvix-store,
using a similar `nix copy` comamnd as above.


```
CH_CMDLINE=init=/nix/store/…-nixos-system-…/init mg run //tvix/boot:runVM --
```

```
  ______      _         ____      _ __
 /_  __/   __(_)  __   /  _/___  (_) /_
  / / | | / / / |/_/   / // __ \/ / __/
 / /  | |/ / />  <   _/ // / / / / /_
/_/   |___/_/_/|_|  /___/_/ /_/_/\__/

2023/09/24 21:16:43 switch_root: moving mounts
2023/09/24 21:16:43 switch_root: Skipping "/run" as the dir does not exist
2023/09/24 21:16:43 switch_root: Changing directory
2023/09/24 21:16:43 switch_root: Moving /
2023/09/24 21:16:43 switch_root: Changing root!
2023/09/24 21:16:43 switch_root: Deleting old /
2023/09/24 21:16:43 switch_root: executing init

<<< NixOS Stage 2 >>>

[    0.322096] booting system configuration /nix/store/g657sdxinpqfcdv0162zmb8vv9b5c4c5-nixos-system-client-23.11.git.82102fc37da
running activation script...
setting up /etc...
starting systemd...
[    0.980740] systemd[1]: systemd 253.6 running in system mode (+PAM +AUDIT -SELINUX +APPARMOR +IMA +SMACK +SECCOMP +GCRYPT -GNUTLS +OPENSSL +ACL +BLKID +CURL +ELFUTILS +FIDO2 +IDN2 -IDN +IPTC +KMOD +LIBCRYPTSETUP +LIBFDISK +PCRE2 -PWQUALITY +P11KIT -QRENCODE +TPM2 +BZIP2 +LZ4 +XZ +ZLIB +ZSTD +BPF_FRAMEWORK -XKBCOMMON +UTMP -SYSVINIT default-hierarchy=unified)
```

This effectively replaces the NixOS Stage 1 entirely.
