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

  # Tracking nixos-unstable as of 2022-01-15.
  unstableHashes = {
    commit = "5aaed40d22f0d9376330b6fa413223435ad6fee5";
    sha256 = "0bs8sr92lzz7mdrlv143draq3j7l42dj69w3px1x31qcr3n5pgcv";
  };

  # Tracking nixos-21.11 as of 2022-01-16.
  stableHashes = {
    commit = "3ddd960a3b575bf3230d0e59f42614b71f9e0db9";
    sha256 = "1sk60h584gfdf5ql3mv4hps6jly6yj6dc5fbkfkh082z1c8885vk";
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
  # allow users to inject their config into builds (e.g. to test CA derivations)
  config =
    (if externalArgs ? nixpkgsConfig then externalArgs.nixpkgsConfig else { })
    // {
      allowUnfree = true;
      allowBroken = true;
    };
  overlays = [
    commitsOverlay
    stableOverlay
    depot.third_party.overlays.haskell
    depot.third_party.overlays.emacs
    depot.third_party.overlays.tvl
    depot.third_party.overlays.ecl-static
  ];
}
