# SPDX-License-Identifier: LGPL-2.1-only
# SPDX-FileCopyrightText: © 2022 The TVL Contributors
# SPDX-FileCopyrightText: © 2004-2022 The Nix Contributors
#
# Execute language tests found in tvix_tests and nix_tests
# using the C++ Nix implementation. Based on NixOS/nix:tests/lang.sh.
{ depot, pkgs, lib, ... }:

let
  testRoot = ../eval/src/tests;

  inherit (pkgs.buildPackages) nix nix_latest;

  parseTest = dir: baseName:
    let
      tokens = builtins.match "(eval|parse)-(okay|fail).+\\.nix" baseName;
    in
    if tokens == null
    then null
    else {
      type = builtins.elemAt tokens 0;
      expectedSuccess = (builtins.elemAt tokens 1) == "okay";
      fileName = "${dir}/${baseName}";
    };

  allLangTests =
    lib.concatMap
      (
        dir:
        lib.pipe
          (builtins.readDir (testRoot + "/${dir}"))
          [
            builtins.attrNames
            (builtins.map (parseTest dir))
            (builtins.filter (t: t != null))
          ]
      ) [ "nix_tests" "nix_tests/notyetpassing" "tvix_tests" ];

  skippedLangTests = {
    # TODO(sterni): set up NIX_PATH in sandbox
    "eval-okay-search-path.nix" = true;
    # Floating point precision differs between tvix and Nix
    "eval-okay-fromjson.nix" = true;
    # C++ Nix can't TCO
    "eval-okay-tail-call-1.nix" = true;
    # Ordering change after 2.3
    "eval-okay-xml.nix" = [ nix ];
    # Missing builtins in Nix 2.3
    "eval-okay-ceil.nix" = [ nix ];
    "eval-okay-floor-ceil.nix" = [ nix ];
    "eval-okay-floor.nix" = [ nix ];
    "eval-okay-groupBy.nix" = [ nix ];
    "eval-okay-zipAttrsWith.nix" = [ nix ];
    # Comparable lists are not in Nix 2.3
    "eval-okay-sort.nix" = [ nix ];
    "eval-okay-compare-lists.nix" = [ nix ];
    # getAttrPos gains support for functionArgs-returned sets after 2.3
    "eval-okay-getattrpos-functionargs.nix" = [ nix ];
    # groupBy appeared (long) after 2.3
    "eval-okay-builtins-groupby-thunk.nix" = [ nix ];
  };

  runCppNixLangTests = cpp-nix:
    let
      testCommand = { fileName, type, expectedSuccess, ... }:
        let
          testBase = lib.removeSuffix ".nix" fileName;
          expFile =
            let
              possibleFiles =
                builtins.filter
                  (path: builtins.pathExists (testRoot + "/${path}"))
                  (builtins.map
                    (ext: "${testBase}.${ext}")
                    [ "exp" "exp.xml" ]);
            in
            if possibleFiles == [ ] then null else builtins.head possibleFiles;
          outFile = "${testBase}.out";

          # Skip if skippedLangTests prescribes it (possibly just for the current nix)
          # or if we are missing an exp file for an eval-okay test.
          skip =
            let
              doSkip = skippedLangTests.${builtins.baseNameOf fileName} or false;
            in
            if type == "eval" && expectedSuccess && (expFile == null) then true
            else if builtins.isBool doSkip then doSkip
            else builtins.any (drv: cpp-nix == drv) doSkip;

          flagsFile = "${testBase}.flags";

          instantiateFlags =
            lib.escapeShellArgs
              (
                [ "--${type}" fileName ]
                ++ lib.optionals (type == "eval") [ "--strict" ]
                ++ lib.optionals (expFile != null && lib.hasSuffix "xml" expFile)
                  [
                    "--no-location"
                    "--xml"
                  ]
              )
            + lib.optionalString (builtins.pathExists (testRoot + "/${flagsFile}"))
              " $(cat '${flagsFile}')";
        in

        if skip
        then "echo \"SKIP ${type} ${fileName}\"\n"
        else ''
          thisTestPassed=true

          echo "RUN  ${type} ${fileName} ${
            lib.optionalString (!expectedSuccess) "(expecting failure)"
          }"

          if ! expect ${if expectedSuccess then "0" else "1"} \
                 nix-instantiate ${instantiateFlags} \
                 ${if expectedSuccess then "1" else "2"}> \
                 ${if expFile != null then outFile else "/dev/null"};
          then
            echo -n "FAIL"
            thisTestPassed=false
          fi
        '' + lib.optionalString (expFile != null) ''
          if ! diff --color=always -u '${outFile}' '${expFile}'; then
            thisTestPassed=false
          fi
        '' + ''
          if $thisTestPassed; then
            echo -n "PASS"
          else
            echo -n "FAIL"
            passed=false
          fi

          echo " ${type} ${fileName}"

          unset thisTestPassed
        '';
    in

    pkgs.stdenv.mkDerivation {
      name = "cpp-${cpp-nix.name}-run-lang-tests";

      nativeBuildInputs = [ cpp-nix ];

      # Obtain tests via the unpackPhase
      src = testRoot;
      dontConfigure = true;

      # Environment expected by the test suite
      TEST_VAR = "foo";

      buildPhase = ''
        # Make nix-instantiate happy in the sandbox
        export NIX_STORE_DIR="$(realpath "$(mktemp -d store.XXXXXXXXXX)")"
        export NIX_STATE_DIR="$(realpath "$(mktemp -d state.XXXXXXXXXX)")"

        # Helper function to check expected exit code
        expect() {
          local expected res
          expected="$1"
          shift
          set +e
          "$@"
          res="$?"
          set -e
          [[ $res -eq $expected ]]
        }

        # Track test results so far
        passed=true

        source "$testCommandsPath"
      '';

      # Actually runs into the argv limit
      passAsFile = [ "testCommands" ];
      testCommands = lib.concatMapStrings testCommand allLangTests;

      installPhase = ''
        if $passed; then
          touch $out
        else
          echo "Some test(s) failed!"
          exit 1
        fi
      '';
    };

in

depot.nix.readTree.drvTargets {
  "nix-2.3" = runCppNixLangTests nix;
  "nix-${lib.versions.majorMinor nix_latest.version}" = runCppNixLangTests nix_latest;
}
