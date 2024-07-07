{ lib, depot, ... }:

{
  mkFeaturePowerset = { crateName, features, override ? { } }:
    let
      powerset = xs:
        let
          addElement = set: element:
            set ++ map (e: [ element ] ++ e) set;
        in
        lib.foldl' addElement [ [ ] ] xs;
    in
    lib.listToAttrs (map
      (featuresPowerset: {
        name = if featuresPowerset != [ ] then "with-features-${lib.concatStringsSep "-" featuresPowerset}" else "no-features";
        value = depot.tvix.crates.workspaceMembers.${crateName}.build.override (old: {
          runTests = true;
          features = featuresPowerset;
        } // (if lib.isFunction override then override old else override)
        );
      })
      (powerset features));

  # Filters the given source, only keeping files related to the build, preventing unnecessary rebuilds.
  # Includes src in the root, all other .rs files and optionally Cargo specific files.
  # Additional files to be included can be specified in extraFileset.
  filterRustCrateSrc =
    { root # The original src
    , extraFileset ? null # Additional filesets to include (e.g. fileFilter for proto files)
    , cargoSupport ? false
    }:
    lib.fileset.toSource {
      inherit root;
      fileset = lib.fileset.intersection
        (lib.fileset.fromSource root) # We build our final fileset from the original src
        (lib.fileset.unions ([
          (root + "/src")
          (lib.fileset.fileFilter (f: f.hasExt "rs") root)
        ] ++ lib.optionals cargoSupport [
          (lib.fileset.fileFilter (f: f.name == "Cargo.toml") root)
          (lib.fileset.maybeMissing (root + "/Cargo.lock"))
        ] ++ lib.optional (extraFileset != null) extraFileset));
    };

  # A function which takes a pkgs instance and returns an overriden defaultCrateOverrides with support for tvix crates.
  # This can be used throughout the rest of the repo.
  defaultCrateOverridesForPkgs = pkgs:
    let
      commonDarwinDeps = with pkgs.darwin.apple_sdk.frameworks; [
        Security
        SystemConfiguration
      ];
    in
    pkgs.defaultCrateOverrides // {
      nix-compat = prev: {
        src = depot.tvix.utils.filterRustCrateSrc rec {
          root = prev.src.origSrc;
          extraFileset = root + "/testdata";
        };
      };
      tvix-build = prev: {
        src = depot.tvix.utils.filterRustCrateSrc rec {
          root = prev.src.origSrc;
          extraFileset = lib.fileset.fileFilter (f: f.hasExt "proto") root;
        };
        PROTO_ROOT = depot.tvix.build.protos.protos;
        nativeBuildInputs = [ pkgs.protobuf ];
        buildInputs = lib.optional pkgs.stdenv.isDarwin commonDarwinDeps;
      };

      tvix-castore = prev: {
        src = depot.tvix.utils.filterRustCrateSrc rec {
          root = prev.src.origSrc;
          extraFileset = lib.fileset.fileFilter (f: f.hasExt "proto") root;
        };
        PROTO_ROOT = depot.tvix.castore.protos.protos;
        nativeBuildInputs = [ pkgs.protobuf ];
      };

      tvix-cli = prev: {
        src = depot.tvix.utils.filterRustCrateSrc rec {
          root = prev.src.origSrc;
          extraFileset = root + "/tests";
        };
        buildInputs = lib.optional pkgs.stdenv.isDarwin commonDarwinDeps;
      };

      tvix-store = prev: {
        src = depot.tvix.utils.filterRustCrateSrc rec {
          root = prev.src.origSrc;
          extraFileset = lib.fileset.fileFilter (f: f.hasExt "proto") root;
        };
        PROTO_ROOT = depot.tvix.store.protos.protos;
        nativeBuildInputs = [ pkgs.protobuf ];
        # fuse-backend-rs uses DiskArbitration framework to handle mount/unmount on Darwin
        buildInputs = lib.optional pkgs.stdenv.isDarwin (commonDarwinDeps ++ pkgs.darwin.apple_sdk.frameworks.DiskArbitration);
      };

      tvix-eval-builtin-macros = prev: {
        src = depot.tvix.utils.filterRustCrateSrc { root = prev.src.origSrc; };
      };

      tvix-eval = prev: {
        src = depot.tvix.utils.filterRustCrateSrc rec {
          root = prev.src.origSrc;
          extraFileset = root + "/proptest-regressions";
        };
      };

      tvix-glue = prev: {
        src = depot.tvix.utils.filterRustCrateSrc {
          root = prev.src.origSrc;
        };
      };

      tvix-serde = prev: {
        src = depot.tvix.utils.filterRustCrateSrc { root = prev.src.origSrc; };
      };

      tvix-tracing = prev: {
        src = depot.tvix.utils.filterRustCrateSrc { root = prev.src.origSrc; };
      };
    };
}
