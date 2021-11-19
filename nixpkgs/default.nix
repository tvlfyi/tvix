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

  # Tracking nixos-unstable as of 2021-11-16.
  unstableHashes = {
    commit = "931ab058daa7e4cd539533963f95e2bb0dbd41e6";
    sha256 = "079925kw711k8agg17kxq36ininpacfjjhgydjy0pqbkhk59v6gm";
  };

  # Tracking nixos-21.05 as of 2021-11-15.
  stableHashes = {
    commit = "46251a79f752ae1d46ef733e8e9760b6d3429da4";
    sha256 = "1xsp0xyrf8arjkf4wi09n96kbg0r8igsmzx8bhc1nj4nr078p0pg";
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
    # Nothing picked from stable presently.
  };

  # Overlay to expose the nixpkgs commits we are using to other Nix code.
  commitsOverlay = _: _: {
    nixpkgsCommits = {
      unstable = unstableHashes.commit;
      stable = stableHashes.commit;
    };
  };
in import nixpkgsSrc {
  config.allowUnfree = true;
  config.allowBroken = true;
  overlays = [
    commitsOverlay
    stableOverlay
    depot.third_party.overlays.haskell
    depot.third_party.overlays.emacs
    depot.third_party.overlays.tvl
    depot.third_party.overlays.ecl-static
  ];
}
