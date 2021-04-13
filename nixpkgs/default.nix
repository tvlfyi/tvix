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

  # Tracking nixos-unstable as of 2021-05-25.
  unstableHashes = {
    commit = "900115a4f7fdd9189e7803ca781a65be663f2c89";
    sha256 = "11551nawxjbgya8sq1p6ghkbws9qz9fbfq3wqawm3zh8ayr4l13j";
  };

  # Tracking nixos-20.09 as of 2021-05-25.
  stableHashes = {
    commit = "ac60476ed94fd5424d9f3410c438825f793a8cbb";
    sha256 = "1dlvpdsy5v09c7rj5f7xgakyj722yqr4415davjpcmrk4n5kw76v";
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
    depot.third_party.overlays.emacs
  ];
}
