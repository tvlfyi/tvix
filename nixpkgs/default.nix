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

  # Tracking nixpkgs master as of 2022-01-12.
  unstableHashes = {
    commit = "47d693f2f6673c5197f9606982b697f840b4dcc6";
    sha256 = "0vwm922ak6s61d09dyayy0inh3r3bgmy9xj8gg55jr79jkl2rab3";
  };

  # Tracking nixos-21.11 as of 2022-01-11.
  stableHashes = {
    commit = "386234e2a61e1e8acf94dfa3a3d3ca19a6776efb";
    sha256 = "1qhfham6vhy67xjdvsmhb3prvsg854wfw4l4avxnvclrcm3k2yg8";
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
