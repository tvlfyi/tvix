# This file imports the pinned nixpkgs sets and applies relevant
# modifications, such as our overlays.
#
# Note that the attribute exposed by this (third_party.nixpkgs) is
# "special" in that the fixpoint used as readTree's config parameter
# in //default.nix passes this attribute as the `pkgs` argument to all
# readTree derivations.

{ depot, externalArgs, ... }:

let
  # This provides the sources of nixpkgs. We track both
  # nixos-unstable, and the current stable channel of the latest NixOS
  # release.

  # Tracking nixos-unstable as of 2021-03-25.
  unstableHashes = {
    commit = "60dd94fb7e01a8288f6638eee71d7cb354c49327";
    sha256 = "0skdwk9bdld295kzrymirs8xrzycqmhsclaz8s18jhcz75hb8sk3";
  };

  # Tracking nixos-20.09 as of 2021-03-25.
  stableHashes = {
    commit = "223d0d733a66b46504ea6b4c15f88b7cc4db58fb";
    sha256 = "073327ris0frqa3kpid3nsjr9w8yx2z83xpsc24w898mrs9r7d5v";
  };

  # import the nixos-unstable package set, or optionally use the
  # source (e.g. a path) specified by the `nixpkgsBisectPath`
  # argument. This is intended for use-cases where the depot is
  # bisected against nixpkgs to find the root cause of an issue in a
  # channel bump.
  nixpkgsSrc = externalArgs.nixpkgsBisectPath or (fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/${unstableHashes.commit}.tar.gz";
    sha256 = unstableHashes.sha256;
  });

  stableNixpkgsSrc = fetchTarball {
    url = "https://github.com/NixOS/nixpkgs/archive/${stableHashes.commit}.tar.gz";
    sha256 = stableHashes.sha256;
  };

  # Stable package set is imported, but not exposed, to overlay
  # required packages into the unstable set.
  stableNixpkgs = import stableNixpkgsSrc {};

  # Overlay for packages that should come from the stable channel
  # instead (e.g. because something is broken in unstable).
  stableOverlay = self: super: {
    inherit (stableNixpkgs)
      awscli # TODO(grfn): Move back to unstable once it is fixed
      ;
  };
in import nixpkgsSrc {
  config.allowUnfree = true;
  config.allowBroken = true;
  overlays = [
    stableOverlay
    depot.third_party.overlays.tvl
    depot.third_party.overlays.haskell
  ];
}
