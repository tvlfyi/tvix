# tvix/tests

This directory provides a bunch of integration tests using tvix.

The currently most interesting (and only) ones ;-) are using a cloud-hypervisor
VM.

## //tvix/tests:test-docs
This is a test encapsulated in a nix build.
It seeds a tvix-store with the tvix docs, then starts a VM, asks it to list all
files in /nix/store, and ensures the store path is present, which acts as a
nice smoketest.

## //tvix/tests:runVM
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

Secondly, configure tvix to use the local backend:

```
export BLOB_SERVICE_ADDR=sled://$PWD/blobs.sled
export DIRECTORY_SERVICE_ADDR=sled://$PWD/directories.sled
export PATH_INFO_SERVICE_ADDR=sled://$PWD/pathinfo.sled
```

Potentially copy some data into tvix-store (via nar-bridge):

```
mg run //tvix:store -- daemon &
mg run //tvix:nar-bridge -- &
rm -Rf ~/.cache/nix; nix copy --to http://localhost:9000\?compression\=none $(mg build //third_party/nixpkgs:hello)
pkill nar-bridge; pkill tvix-store
```

#### Interactive shell
Run the VM like this:

```
CH_CMDLINE=tvix.shell mg run //tvix/tests:runVM --
```

You'll get dropped into a shell, from which you can list the store contents:

```
[    0.282381] Run /init as init process
2023/09/24 13:03:38 Welcome to u-root!
                              _
   _   _      _ __ ___   ___ | |_
  | | | |____| '__/ _ \ / _ \| __|
  | |_| |____| | | (_) | (_) | |_
   \__,_|    |_|  \___/ \___/ \__|

2023/09/24 13:03:38 Running tvix-init…
2023/09/24 13:03:38 Creating /nix/store
2023/09/24 13:03:38 Mounting…
2023/09/24 13:03:38 Invoking shell
…
/# ls -la /nix/store/
dr-xr-xr-x root 0 0   Jan  1 00:00 .
dr-xr-xr-x root 0 989 Jan  1 00:00 aw2fw9ag10wr9pf0qk4nk5sxi0q0bn56-glibc-2.37-8
dr-xr-xr-x root 0 3   Jan  1 00:00 jbwb8d8l28lg9z0xzl784wyb9vlbwss6-xgcc-12.3.0-libgcc
dr-xr-xr-x root 0 82  Jan  1 00:00 k8ivghpggjrq1n49xp8sj116i4sh8lia-libidn2-2.3.4
dr-xr-xr-x root 0 141 Jan  1 00:00 mdi7lvrn2mx7rfzv3fdq3v5yw8swiks6-hello-2.12.1
dr-xr-xr-x root 0 5   Jan  1 00:00 s2gi8pfjszy6rq3ydx0z1vwbbskw994i-libunistring-1.1
```

Once you're done, run `poweroff` to turn off the VM.

#### Execute a specific binary
Run the VM like this:

```
hello_cmd=$(mg build //third_party/nixpkgs:hello)/bin/hello
CH_CMDLINE=tvix.exec=$hello_cmd mg run //tvix/tests:runVM --
```

Observe it executing the file (and closure) from the tvix-store:

```
2023/09/24 13:06:13 Welcome to u-root!
                              _
   _   _      _ __ ___   ___ | |_
  | | | |____| '__/ _ \ / _ \| __|
  | |_| |____| | | (_) | (_) | |_
   \__,_|    |_|  \___/ \___/ \__|

2023/09/24 13:06:13 Running tvix-init…
2023/09/24 13:06:13 Creating /nix/store
2023/09/24 13:06:13 Mounting…
2023/09/24 13:06:13 Invoking /nix/store/mdi7lvrn2mx7rfzv3fdq3v5yw8swiks6-hello-2.12.1/bin/hello
…
Hello, world!
```