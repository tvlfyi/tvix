{ depot, pkgs, lib, ... }:

lib.fix (self:
depot.third_party.naersk.buildPackage {
  src = depot.third_party.gitignoreSource ./.;
  # see https://github.com/nix-community/naersk/issues/169
  root = depot.nix.sparseTree ./. [ ./Cargo.lock ./Cargo.toml ];

  doCheck = true;
  cargoBuildOptions = opts: opts ++ [ "--all-targets" ];

  meta.ci.targets = builtins.attrNames self.passthru;

  passthru.cpp-nix-run-lang-tests = pkgs.stdenv.mkDerivation {
    name = "cpp-nix-run-lang-tests";

    src = ./src/tests;
    dontConfigure = true;

    nativeBuildInputs = [
      pkgs.buildPackages.nix
    ];

    buildPhase = ''
      chmod +x $scriptPath
      patchShebangs --build $scriptPath

      mkdir store var
      export NIX_STORE_DIR="$(realpath store)"
      export NIX_STATE_DIR="$(realpath var)"

      $scriptPath
    '';

    installPhase = "touch $out";

    passAsFile = [ "script" ];
    script = ''
      #!/usr/bin/env bash
      # SPDX-License-Identifier: LGPL-2.1-only
      # SPDX-FileCopyrightText: © 2022 The TVL Contributors
      # SPDX-FileCopyrightText: © 2004-2022 The Nix Contributors
      #
      # Execute language tests found in tvix_tests and nix_tests
      # using the C++ Nix implementation. Based on NixOS/nix:tests/lang.sh.

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

      TESTDIR="''${1:-.}"

      fail=0

      for i in "$TESTDIR/"*_tests/parse-fail-*.nix; do
          echo "parsing $i (should fail)";
          if ! expect 1 nix-instantiate --parse - < $i 2> /dev/null; then
              echo "FAIL: $i shouldn't parse"
              fail=1
          fi
      done

      for i in "$TESTDIR/"*_tests/parse-okay-*.nix; do
          echo "parsing $i (should succeed)";
          if ! expect 0 nix-instantiate --parse - < $i > /dev/null; then
              echo "FAIL: $i should parse"
              fail=1
          fi
      done

      for i in "$TESTDIR/"*_tests/eval-fail-*.nix; do
          echo "evaluating $i (should fail)";
          if ! expect 1 nix-instantiate --eval $i 2> /dev/null; then
              echo "FAIL: $i shouldn't evaluate"
              fail=1
          fi
      done

      export TEST_VAR="foo"

      for i in "$TESTDIR/"*_tests/eval-okay-*.nix; do
          echo "evaluating $i (should succeed)";

          base="$(dirname "$i")/$(basename "$i" ".nix")"

          if [[ "$(basename "$i")" == "eval-okay-search-path.nix" ]]; then
            # TODO(sterni): fix this test
            echo "SKIPPED: $i"
            continue
          fi

          if test -e $base.exp; then
              flags=
              if test -e $base.flags; then
                  flags=$(cat $base.flags)
              fi
              if ! expect 0 nix-instantiate $flags --eval --strict $base.nix > $base.out; then
                  echo "FAIL: $i should evaluate"
                  fail=1
              elif ! diff $base.out $base.exp; then
                  echo "FAIL: evaluation result of $i not as expected"
                  fail=1
              fi
          fi

          if test -e $base.exp.xml; then
              if ! expect 0 nix-instantiate --eval --xml --no-location --strict \
                      $base.nix > $base.out.xml; then
                  echo "FAIL: $i should evaluate"
                  fail=1
              elif ! cmp -s $base.out.xml $base.exp.xml; then
                  echo "FAIL: XML evaluation result of $i not as expected"
                  fail=1
              fi
          fi
      done

      exit $fail
    '';
  };
}
)
