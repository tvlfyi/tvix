Nix builtins
============

Nix has a lot of built-in functions, some of which are accessible in
the global scope, and some of which are only accessible through the
global `builtins` attribute set.

This document is an attempt to track all of these builtins, but
without documenting their functionality.

See also https://nixos.org/manual/nix/stable/expressions/builtins.html

| name                          | global | arity | pure  |
|-------------------------------|--------|-------|-------|
| abort                         | true   | 1     |       |
| add                           | false  | 2     | true  |
| addErrorContext               | false  | ?     |       |
| all                           | false  | 2     | true  |
| any                           | false  | 2     | true  |
| appendContext                 | false  | ?     |       |
| attrNames                     | false  | 1     | true  |
| attrValues                    | false  |       | true  |
| baseNameOf                    | true   |       |       |
| bitAnd                        | false  |       |       |
| bitOr                         | false  |       |       |
| bitXor                        | false  |       |       |
| builtins                      | true   |       |       |
| catAttrs                      | false  |       |       |
| compareVersions               | false  |       |       |
| concatLists                   | false  |       |       |
| concatMap                     | false  |       |       |
| concatStringsSep              | false  |       |       |
| currentSystem                 | false  |       |       |
| currentTime                   | false  |       |       |
| deepSeq                       | false  |       |       |
| derivation                    | true   |       |       |
| derivationStrict              | true   |       |       |
| dirOf                         | true   |       |       |
| div                           | false  |       |       |
| elem                          | false  |       |       |
| elemAt                        | false  |       |       |
| false                         | true   |       |       |
| fetchGit                      | true   |       |       |
| fetchMercurial                | true   |       |       |
| fetchTarball                  | true   |       |       |
| fetchurl                      | false  |       |       |
| filter                        | false  |       |       |
| filterSource                  | false  |       |       |
| findFile                      | false  |       |       |
| foldl'                        | false  |       |       |
| fromJSON                      | false  |       |       |
| fromTOML                      | true   |       |       |
| functionArgs                  | false  |       |       |
| genList                       | false  |       |       |
| genericClosure                | false  |       |       |
| getAttr                       | false  |       |       |
| getContext                    | false  |       |       |
| getEnv                        | false  |       | false |
| hasAttr                       | false  |       |       |
| hasContext                    | false  |       |       |
| hashFile                      | false  |       |       |
| hashString                    | false  |       |       |
| head                          | false  |       |       |
| import                        | true   |       |       |
| intersectAttrs                | false  |       |       |
| isAttrs                       | false  |       |       |
| isBool                        | false  |       |       |
| isFloat                       | false  |       |       |
| isFunction                    | false  |       |       |
| isInt                         | false  |       |       |
| isList                        | false  |       |       |
| isNull                        | true   |       |       |
| isPath                        | false  |       |       |
| isString                      | false  |       |       |
| langVersion                   | false  |       |       |
| length                        | false  |       |       |
| lessThan                      | false  |       |       |
| listToAttrs                   | false  |       |       |
| map                           | true   |       |       |
| mapAttrs                      | false  |       |       |
| match                         | false  |       |       |
| mul                           | false  |       |       |
| nixPath                       | false  |       |       |
| nixVersion                    | false  |       |       |
| null                          | true   |       |       |
| parseDrvName                  | false  |       |       |
| partition                     | false  |       |       |
| path                          | false  |       |       |
| pathExists                    | false  |       |       |
| placeholder                   | true   |       |       |
| readDir                       | false  |       |       |
| readFile                      | false  |       |       |
| removeAttrs                   | true   |       |       |
| replaceStrings                | false  |       |       |
| scopedImport                  | true   |       |       |
| seq                           | false  |       |       |
| sort                          | false  |       |       |
| split                         | false  |       |       |
| splitVersion                  | false  |       |       |
| storeDir                      | false  |       |       |
| storePath                     | false  |       |       |
| stringLength                  | false  |       |       |
| sub                           | false  |       |       |
| substring                     | false  |       |       |
| tail                          | false  |       |       |
| throw                         | true   |       |       |
| toFile                        | false  |       |       |
| toJSON                        | false  |       |       |
| toPath                        | false  |       |       |
| toString                      | true   |       |       |
| toXML                         | false  |       |       |
| trace                         | false  |       |       |
| true                          | true   |       |       |
| tryEval                       | false  |       |       |
| typeOf                        | false  |       |       |
| unsafeDiscardOutputDependency | false  |       |       |
| unsafeDiscardStringContext    | false  |       |       |
| unsafeGetAttrPos              | false  |       |       |
| valueSize                     | false  |       |       |

## Added after C++ Nix 2.3 (without Flakes enabled)

| name          | global | arity | pure  |
|---------------|--------|-------|-------|
| break         | false  | 1     |       |
| ceil          | false  | 1     | true  |
| fetchTree     | true   | 1     |       |
| floor         | false  | 1     | true  |
| groupBy       | false  | 2     | true  |
| traceVerbose  | false  | 2     |       |
| zipAttrsWith  | false  | 2     | true  |
