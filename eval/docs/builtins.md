Nix builtins
============

Nix has a lot of built-in functions, some of which are accessible in
the global scope, and some of which are only accessible through the
global `builtins` attribute set.

This document is an attempt to track all of these builtins, but
without documenting their functionality.

See also https://nixos.org/manual/nix/stable/expressions/builtins.html

The `impl` column indicates implementation status in tvix:
- implemented: "" (empty cell)
- not yet implemented, but not blocked: `todo`
- not yet implemented, but blocked by other prerequisites:
  - `store`: awaiting eval<->store api(s)
  - `context`: awaiting support for string contexts

| name                          | global | arity | pure  | impl    |
|-------------------------------|--------|-------|-------|---------|
| abort                         | true   | 1     |       |         |
| add                           | false  | 2     | true  |         |
| addErrorContext               | false  | ?     |       | context |
| all                           | false  | 2     | true  |         |
| any                           | false  | 2     | true  |         |
| appendContext                 | false  | ?     |       | context |
| attrNames                     | false  | 1     | true  |         |
| attrValues                    | false  |       | true  |         |
| baseNameOf                    | true   |       |       |         |
| bitAnd                        | false  |       |       |         |
| bitOr                         | false  |       |       |         |
| bitXor                        | false  |       |       |         |
| builtins                      | true   |       |       |         |
| catAttrs                      | false  |       |       |         |
| compareVersions               | false  |       |       |         |
| concatLists                   | false  |       |       |         |
| concatMap                     | false  |       |       |         |
| concatStringsSep              | false  |       |       |         |
| currentSystem                 | false  |       |       |         |
| currentTime                   | false  |       | false |         |
| deepSeq                       | false  |       |       |         |
| derivation                    | true   |       |       | store   |
| derivationStrict              | true   |       |       | store   |
| dirOf                         | true   |       |       |         |
| div                           | false  |       |       |         |
| elem                          | false  |       |       |         |
| elemAt                        | false  |       |       |         |
| false                         | true   |       |       |         |
| fetchGit                      | true   |       |       | store   |
| fetchMercurial                | true   |       |       | store   |
| fetchTarball                  | true   |       |       | store   |
| fetchurl                      | false  |       |       | store   |
| filter                        | false  |       |       |         |
| filterSource                  | false  |       |       | store   |
| findFile                      | false  |       | false | todo    |
| foldl'                        | false  |       |       |         |
| fromJSON                      | false  |       |       |         |
| fromTOML                      | true   |       |       |         |
| functionArgs                  | false  |       |       |         |
| genList                       | false  |       |       |         |
| genericClosure                | false  |       |       | todo    |
| getAttr                       | false  |       |       |         |
| getContext                    | false  |       |       |         |
| getEnv                        | false  |       | false |         |
| hasAttr                       | false  |       |       |         |
| hasContext                    | false  |       |       |         |
| hashFile                      | false  |       | false | todo    |
| hashString                    | false  |       |       | todo    |
| head                          | false  |       |       |         |
| import                        | true   |       |       |         |
| intersectAttrs                | false  |       |       |         |
| isAttrs                       | false  |       |       |         |
| isBool                        | false  |       |       |         |
| isFloat                       | false  |       |       |         |
| isFunction                    | false  |       |       |         |
| isInt                         | false  |       |       |         |
| isList                        | false  |       |       |         |
| isNull                        | true   |       |       |         |
| isPath                        | false  |       |       |         |
| isString                      | false  |       |       |         |
| langVersion                   | false  |       |       |         |
| length                        | false  |       |       |         |
| lessThan                      | false  |       |       |         |
| listToAttrs                   | false  |       |       |         |
| map                           | true   |       |       |         |
| mapAttrs                      | false  |       |       |         |
| match                         | false  |       |       |         |
| mul                           | false  |       |       |         |
| nixPath                       | false  |       |       | todo    |
| nixVersion                    | false  |       |       | todo    |
| null                          | true   |       |       |         |
| parseDrvName                  | false  |       |       |         |
| partition                     | false  |       |       |         |
| path                          | false  |       | sometimes | store |
| pathExists                    | false  |       | false |         |
| placeholder                   | true   |       |       | context |
| readDir                       | false  |       | false |         |
| readFile                      | false  |       | false |         |
| removeAttrs                   | true   |       |       |         |
| replaceStrings                | false  |       |       |         |
| scopedImport                  | true   |       |       |         |
| seq                           | false  |       |       |         |
| sort                          | false  |       |       |         |
| split                         | false  |       |       |         |
| splitVersion                  | false  |       |       |         |
| storeDir                      | false  |       |       | store   |
| storePath                     | false  |       |       | store   |
| stringLength                  | false  |       |       |         |
| sub                           | false  |       |       |         |
| substring                     | false  |       |       |         |
| tail                          | false  |       |       |         |
| throw                         | true   |       |       |         |
| toFile                        | false  |       |       | store   |
| toJSON                        | false  |       |       | todo    |
| toPath                        | false  |       |       |         |
| toString                      | true   |       |       |         |
| toXML                         | true   |       |       |         |
| trace                         | false  |       |       |         |
| true                          | true   |       |       |         |
| tryEval                       | false  |       |       |         |
| typeOf                        | false  |       |       |         |
| unsafeDiscardOutputDependency | false  |       |       | context |
| unsafeDiscardStringContext    | false  |       |       |         |
| unsafeGetAttrPos              | false  |       |       | todo    |
| valueSize                     | false  |       |       | todo    |

## Added after C++ Nix 2.3 (without Flakes enabled)

| name          | global | arity | pure  | impl  |
|---------------|--------|-------|-------|-------|
| break         | false  | 1     |       | todo  |
| ceil          | false  | 1     | true  |       |
| fetchTree     | true   | 1     |       | todo  |
| floor         | false  | 1     | true  |       |
| groupBy       | false  | 2     | true  |       |
| traceVerbose  | false  | 2     |       | todo  |
| zipAttrsWith  | false  | 2     | true  | todo  |
