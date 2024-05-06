
# TOC

| Operation                                                   | Id |
| ----------------------------------------------------------- | -- |
| [IsValidPath](#isvalidpath)                                 | 1  |
| [HasSubstitutes](#hassubstitutes)                           | 3  |
| [QueryReferrers](#queryreferrers)                           | 6  |
| [AddToStore](#addtostore)                                   | 7  |
| [BuildPaths](#buildpaths)                                   | 9  |
| [EnsurePath](#ensurepath)                                   | 10 |
| [AddTempRoot](#addtemproot)                                 | 11 |
| [AddIndirectRoot](#addindirectroot)                         | 12 |
| [FindRoots](#findroots)                                     | 14 |
| [SetOptions](#setoptions)                                   | 19 |
| [CollectGarbage](#collectgarbage)                           | 20 |
| [QuerySubstitutablePathInfo](#querysubstitutablepathinfo)   | 21 |
| [QueryAllValidPaths](#queryallvalidpaths)                   | 23 |
| [QueryPathInfo](#querypathinfo)                             | 26 |
| [QueryPathFromHashPart](#querypathfromhashpart)             | 29 |
| [QuerySubstitutablePathInfos](#querysubstitutablepathinfos) | 30 |
| [QueryValidPaths](#queryvalidpaths)                         | 31 |
| [QuerySubstitutablePaths](#querysubstitutablepaths)         | 32 |
| [QueryValidDerivers](#queryvalidderivers)                   | 33 |
| [OptimiseStore](#optimisestore)                             | 34 |
| [VerifyStore](#verifystore)                                 | 35 |
| [BuildDerivation](#buildderivation)                         | 36 |
| [AddSignatures](#addsignatures)                             | 37 |
| [NarFromPath](#narfrompath)                                 | 38 |
| [AddToStoreNar](#addtostore)                                | 39 |
| [QueryMissing](#querymissing)                               | 40 |
| [QueryDerivationOutputMap](#queryderivationoutputmap)       | 41 |
| [RegisterDrvOutput](#registerdrvoutput)                     | 42 |
| [QueryRealisation](#queryrealisation)                       | 43 |
| [AddMultipleToStore](#addmultipletostore)                   | 44 |
| [AddBuildLog](#addbuildlog)                                 | 45 |
| [BuildPathsWithResults](#buildpathswithresults)             | 46 |
| [AddPermRoot](#addpermroot)                                 | 47 |


## Obsolete operations

| Operation                                                 | Id |
| --------------------------------------------------------- | -- |
| [QueryPathHash](#querypathhash)                           | 4  |
| [QueryReferences](#queryreferences)                       | 5  |
| [AddTextToStore](#addtexttostore)                         | 8  |
| [SyncWithGC](#syncwithgc)                                 | 13 |
| [ExportPath](#exportpath)                                 | 16 |
| [QueryDeriver](#queryderiver)                             | 18 |
| [QueryDerivationOutputs](#queryderivationoutputs)         | 22 |
| [ImportPaths](#importpaths)                               | 27 |
| [QueryDerivationOutputNames](#queryderivationoutputnames) | 28 |


## Removed operations

| Operation                                         | Id |
| ------------------------------------------------- | -- |
| [Quit](#quit-removed)                             | 0  |
| [ImportPath](#importpath-removed)                 | 17 |
| [Old CollectGarbage](#old-collectgarbage-removed) | 15 |
| [QueryFailedPaths](#queryfailedpaths)             | 24 |
| [ClearFailedPaths](#clearfailedpaths)             | 25 |



## Quit (removed)

**Id:** 0<br>
**Introduced:** Nix 0.11<br>
**Removed:** Became dead code in Nix 0.11 and removed in Nix 1.8


## IsValidPath

**Id:** 1<br>
**Introduced:** Nix 0.11<br>

As the name says checks that a store path is valid i.e. in the store.

This is a pretty core operation used everywhere.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
isValid :: [Bool][se-Bool]


## HasSubstitutes

**Id:** 3<br>
**Introduced:** Nix 0.11<br>

Checks if we can substitute the input path from a substituter. Uses
QuerySubstitutablePaths under the hood :/

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
hasSubstitutes :: [Bool][se-Bool]


## QueryPathHash

**Id:** 4<br>
**Introduced:** Nix 0.11<br>
**Obsolete:** Protocol 1.16, Nix 2.0<br>

Retrieves the base16 NAR hash of a given store path.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
hash :: [String][se-String] (base16-encoded NAR hash without algorithm prefix)


## QueryReferences

**Id:** 5<br>
**Introduced:** Nix 0.11<br>
**Obsolete:** Protocol 1.16, Nix 2.0<br>

Retrieves the references of a given path

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
references :: [List][se-List] of [StorePath][se-StorePath]


## QueryReferrers

**Id:** 6<br>
**Introduced:** Nix 0.11<br>

Retrieves the referrers of a given path.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
referrers :: [List][se-List] of [StorePath][se-StorePath]


## AddToStore

**Id:** 7<br>
**Introduced:** Nix 0.11<br>

Add a new path to the store.

### Before protocol version 1.25
#### Inputs
- baseName :: [String][se-String]
- fixed :: [Bool64][se-Bool64]
- recursive :: [FileIngestionMethod][se-FileIngestionMethod]
- hashAlgo :: [String][se-String]
- NAR dump

If fixed is `true`, hashAlgo is forced to `sha256` and recursive is forced to
`Recursive`.

Only `Flat` and `Recursive` values are supported for the recursive input
parameter.

#### Outputs
path :: [StorePath][se-StorePath]

### Protocol version 1.25 or newer
#### Inputs
- name :: [String][se-String]
- camStr :: [ContentAddressMethodWithAlgo][se-ContentAddressMethodWithAlgo]
- refs :: [List][se-List] of [StorePath][se-StorePath]
- repairBool :: [Bool64][se-Bool64]
- [Framed][se-Framed] NAR dump

#### Outputs
info :: [ValidPathInfo][se-ValidPathInfo]


## AddTextToStore

**Id:** 8<br>
**Introduced:** Nix 0.11<br>
**Obsolete:** Protocol 1.25, Nix 2.4

Add a text file as a store path.

This was obsoleted by adding the functionality implemented by this operation
to [AddToStore](#addtostore). And so this corresponds to calling
[AddToStore](#addtostore) with `camStr` set to `text:sha256` and `text`
wrapped as a NAR.

### Inputs
- suffix :: [String][se-String]
- text :: [Bytes][se-Bytes]
- refs :: [List][se-List] of [StorePath][se-StorePath]

### Outpus
path :: [StorePath][se-StorePath]


## BuildPaths

**Id:** 9<br>
****Introduced:**** Nix 0.11<br>

Build (or substitute) a list of derivations.

### Inputs
paths :: [List][se-List] of [DerivedPath][se-DerivedPath]

#### Protocol 1.15 or newer
mode :: [BuildMode][se-BuildMode]

Check that connection is trusted before allowing Repair mode.

### Outputs
1 :: [Int][se-Int] (hardcoded)


## EnsurePath

**Id:** 10<br>
**Introduced:** Nix 0.11<br>

Checks if a path is valid. Note: it may be made valid by running a substitute.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
1 :: [Int][se-Int] (hardcoded)


## AddTempRoot

**Id:** 11<br>
**Introduced:** Nix 0.11<br>

Creates a temporary GC root for the given store path.

Temporary GC roots are valid only for the life of the connection and are used
primarily to prevent the GC from pulling the rug out from under the client and
deleting store paths that the client is actively doing something with.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
1 :: [Int][se-Int] (hardcoded)


## AddIndirectRoot

**Id:** 12<br>
**Introduced:** Nix 0.11<br>

Add an indirect root, which is a weak reference to the user-facing symlink
created by [AddPermRoot](#addpermroot).

Only ever sent on the local unix socket nix daemon.

### Inputs
path :: [String][se-String]

### Outputs
1 :: [Int][se-Int] (hardcoded)


## SyncWithGC

**Id:** 13<br>
**Introduced:** Nix 0.11<br>
**Obsolete:** Protocol 1.32, Nix 2.5.0

Acquire the global GC lock, then immediately release it.  This function must be
called after registering a new permanent root, but before exiting.  Otherwise,
it is possible that a running garbage collector doesn't see the new root and
deletes the stuff we've just built.  By acquiring the lock briefly, we ensure
that either:

- The collector is already running, and so we block until the
    collector is finished.  The collector will know about our
    *temporary* locks, which should include whatever it is we
    want to register as a permanent lock.
- The collector isn't running, or it's just started but hasn't
    acquired the GC lock yet.  In that case we get and release
    the lock right away, then exit.  The collector scans the
    permanent root and sees ours.

In either case the permanent root is seen by the collector.

Was made obsolete by using [AddTempRoot](#addtemproot) to accomplish the same
thing.


## FindRoots

**Id:** 14<br>
**Introduced:** Nix 0.11<br>

Find the GC roots.

### Outputs
roots :: [Map][se-Map] of [String][se-String] to [StorePath][se-StorePath]

The key is the link pointing to the given store path.


## Old CollectGarbage (removed)

**Id:** 15<br>
**Introduced:** Nix 0.11<br>
**Removed:** Protocol 1.02, Nix 0.12<br>


## ExportPath

**Id:** 16<br>
**Introduced:** Nix 0.11<br>
**Obsolete:** Protocol 1.17, Nix 2.0<br>

Export a store path in the binary format nix-store --import expects. See implementation there https://github.com/NixOS/nix/blob/db3bf180a569cb20db42c5e4669d2277be6f46b6/src/libstore/export-import.cc#L29 for more details.

### Inputs
- path :: [StorePath][se-StorePath]
- sign :: [Int][se-Int] (ignored and hardcoded to 0 in client)

### Outputs
Uses `STDERR_WRITE` to send dump in export format

After dump it outputs.

1 :: [Int][se-Int] (hardcoded)


## ImportPath (removed)

**Id:** 17<br>
**Introduced:** Nix 0.11<br>
**Removed:** Protocol 1.09, Nix 1.0<br>


## QueryDeriver

**Id:** 18<br>
**Introduced:** Nix 0.11<br>
**Obsolete:** Protocol 1.16, Nix 2.0<br>

Returns the store path of the derivation for a given store path.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
deriver :: [OptStorePath][se-OptStorePath]


## SetOptions

**Id:** 19<br>
**Introduced:** Nix 0.11<br>

Sends client options to the remote side.

Only ever used right after the handshake.

### Inputs

- keepFailed :: [Int][se-Int]
- keepGoing :: [Int][se-Int]
- tryFallback :: [Int][se-Int]
- verbosity :: [Verbosity][se-Verbosity]
- maxbuildJobs :: [Int][se-Int]
- maxSilentTime :: [Int][se-Int]
- useBuildHook :: [Bool][se-Bool] (ignored and hardcoded to true in client)
- verboseBuild :: [Verbosity][se-Verbosity]
- logType :: [Int][se-Int] (ignored and hardcoded to 0 in client)
- printBuildTrace :: [Int][se-Int] (ignored and hardcoded to 0 in client)
- buildCores :: [Int][se-Int]
- useSubstitutes :: [Int][se-Int]

### Protocol 1.12 or newer
otherSettings :: [Map][se-Map] of [String][se-String] to [String][se-String]


## CollectGarbage

**Id:** 20<br>
**Introduced:** Protocol 1.02, Nix 0.12<br>

Find the GC roots.

### Inputs
- action :: [GCAction][se-GCAction]
- pathsToDelete :: [List][se-List] of [StorePath][se-StorePath]
- ignoreLiveness :: [Bool64][se-Bool64]
- maxFreed :: [UInt64][se-UInt64]
- removed :: [Int][se-Int] (ignored and hardcoded to 0 in client)
- removed :: [Int][se-Int] (ignored and hardcoded to 0 in client)
- removed :: [Int][se-Int] (ignored and hardcoded to 0 in client)

### Outputs
- pathsDeleted :: [List][se-List] of [String][se-String]
- bytesFreed :: [UInt64][se-UInt64]
- 0 :: [UInt64][se-UInt64] (hardcoded)

Depending on the action pathsDeleted is, the GC roots, or the paths that would
be or have been deleted.


## QuerySubstitutablePathInfo

**Id:** 21<br>
**Introduced:** Protocol 1.02, Nix 0.12<br>

Retrieves the various substitutable paths infos for a given path.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
found :: [Bool][se-Bool]

#### If found is true
- deriver :: [OptStorePath][se-OptStorePath]
- references :: [List][se-List] of [StorePath][se-StorePath]
- downloadSize :: [UInt64][se-UInt64]
- narSize :: [UInt64][se-UInt64]


## QueryDerivationOutputs

**Id:** 22<br>
**Introduced:** Protocol 1.05, Nix 1.0<br>
**Obsolete:** Protocol 1.22*, Nix 2.4<br>

Retrieves all the outputs paths of a given derivation.

### Inputs
path :: [StorePath][se-StorePath] (must point to a derivation)

### Outputs
derivationOutputs :: [List][se-List] of [StorePath][se-StorePath]


## QueryAllValidPaths

**Id:** 23<br>
**Introduced:** Protocol 1.05, Nix 1.0<br>

Retrieves all the valid paths contained in the store.

### Outputs
paths :: [List][se-List] of [StorePath][se-StorePath]


## QueryFailedPaths (removed)

**Id:** 24<br>
**Introduced:** Protocol 1.05, Nix 1.0<br>
**Removed:** Protocol 1.16, Nix 2.0<br>

Failed build caching API only ever used by Hydra.


## ClearFailedPaths (removed)

**Id:** 25<br>
**Introduced:** Protocol 1.05, Nix 1.0<br>
**Removed:** Protocol 1.16, Nix 2.0<br>

Failed build caching API only ever used by Hydra.


## QueryPathInfo

**Id:** 26<br>
**Introduced:** Protocol 1.06, Nix 1.0<br>

Retrieves the pathInfo for a given path.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs

#### If protocol version is 1.17 or newer
success :: [Bool64][se-Bool64]

##### If success is true
pathInfo :: [UnkeyedValidPathInfo][se-UnkeyedValidPathInfo]

#### If protocol version is older than 1.17
If info not found return error with `STDERR_ERROR`

pathInfo :: [UnkeyedValidPathInfo][se-UnkeyedValidPathInfo]


## ImportPaths

**Id:** 27<br>
**Introduced:** Protocol 1.09, Nix 1.0<br>
**Obsolete:** Protocol 1.17, Nix 2.0<br>

Older way of adding a store path to the remote store.

It was obsoleted and replaced by AddToStoreNar because it sends the NAR
before the metadata about the store path and so you would typically have
to store the NAR in memory or temporarily on disk before processing it.

### Inputs
List of NAR dumps coming from the ExportPaths operations.

### Outputs
importedPaths :: [List][se-List] of [StorePath][se-StorePath]


## QueryDerivationOutputNames

**Id:** 28<br>
**Introduced:** Protocol 1.08, Nix 1.0<br>
**Obsolete:** Protocol 1.21, Nix 2.4<br>

Retrieves the name of the outputs of a given derivation. EG. out, dev, etc.

### Inputs
path :: [StorePath][se-StorePath] (must be a derivation path)

### Outputs
names :: [List][se-List] of [String][se-String]


## QueryPathFromHashPart

**Id:** 29<br>
**Introduced:** Protocol 1.11, Nix 1.1<br>

Retrieves a store path from a base16 (input) hash. Returns "" if no path was
found.

### Inputs
hashPart :: [String][se-String]  (must be a base-16 hash)

### Outputs
path :: [OptStorePath][se-OptStorePath]


## QuerySubstitutablePathInfos

**Id:** 30<br>
**Introduced:** Protocol 1.12*, Nix 1.2<br>

Retrieves the various substitutable paths infos for set of store paths.

### Inputs
#### If protocol version is 1.22 or newer
paths :: [Map][se-Map] of [StorePath][se-StorePath] to [OptContentAddress][se-OptContentAddress] 

#### If protocol version older than 1.22
paths :: [List][se-List] of [StorePath][se-StorePath]

### Outputs
infos :: [List][se-List] of [SubstitutablePathInfo][se-SubstitutablePathInfo]


## QueryValidPaths

**Id:** 31<br>
**Introduced:** Protocol 1.12, Nix 1.2<br>

Takes a list of store paths and returns a new list only containing the valid store paths

## Inputs
paths :: [List][se-List] of [StorePath][se-StorePath]

### If protocol version is 1.27 or newer
substitute :: [Bool][se-Bool] (defaults to false if not sent)

## Outputs
paths :: [List][se-List] of [StorePath][se-StorePath]


## QuerySubstitutablePaths

**Id:** 32<br>
**Introduced:** Protocol 1.12, Nix 1.2<br>

Takes a set of store path, returns a filtered new set of paths that can be
substituted.

In versions of the protocol prior to 1.12 [HasSubstitutes](#hassubstitutes)
is used to implement the functionality that this operation provides.

### Inputs
paths :: [List][se-List] of [StorePath][se-StorePath]

### Outputs
paths :: [List][se-List] of [StorePath][se-StorePath]


## QueryValidDerivers

**Id:** 33<br>
**Introduced:** Protocol 1.13*, Nix 1.3<br>

Retrieves the derivers of a given path.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
derivers :: [List][se-List] of [StorePath][se-StorePath]


## OptimiseStore

**Id:** 34<br>
**Introduced:** Protocol 1.14, Nix 1.8<br>

Optimise store by hardlinking files with the same content.

### Outputs
1 :: [Int][se-Int] (hardcoded)


## VerifyStore

**Id:** 35<br>
**Introduced:** Protocol 1.14, Nix 1.9<br>

Verify store either only db and existence of path or entire contents of store
paths against the NAR hash. 

### Inputs
- checkContents :: [Bool64][se-Bool64]
- repair :: [Bool64][se-Bool64]

### Outputs
errors :: [Bool][se-Bool]


## BuildDerivation

**Id:** 36<br>
**Introduced:** Protocol 1.14, Nix 1.10<br>

Main build operation used when remote building.

When functioning as a remote builder this operation is used instead of
BuildPaths so that it doesn't have to send the entire tree of derivations
to the remote side first before it can start building. What this does
instead is have a reduced version of the derivation to be built sent as
part of its input and then only building that derivation.

The paths required by the build need to be part of the remote store
(by copying with AddToStoreNar or substituting) before this operation is
called.

### Inputs
- drvPath :: [StorePath][se-StorePath]
- drv :: [BasicDerivation][se-BasicDerivation]
- buildMode :: [BuildMode][se-BuildMode]

### Outputs
buildResult :: [BuildResult][se-BuildResult]


## AddSignatures

**Id:** 37<br>
**Introduced:** Protocol 1.16, Nix 2.0<br>

Add the signatures associated to a given path.

### Inputs
- path :: [StorePath][se-StorePath]
- signatures :: [List][se-List] of [String][se-String]

### Outputs
1 :: [Int][se-Int] (hardcoded)


## NarFromPath

**Id:** 38<br>
**Introduced:** Protocol 1.17, Nix 2.0<br>

Main way of getting the contents of a store path to the client.

As the name suggests this is done by sending a NAR file.

It replaced the now obsolete ExportPath operation and is used by newer clients to
implement the export functionality for cli. It is also used when remote building
to transfer build results from remote builder to client.

### Inputs
path :: [StorePath][se-StorePath]

### Outputs
NAR dumped straight to the stream.


## AddToStoreNar

**Id:** 39<br>
**Introduced:** Protocol 1.17, Nix 2.0<br>

Dumps a path as a NAR

### Inputs
- path :: [StorePath][se-StorePath]
- deriver :: [OptStorePath][se-OptStorePath]
- narHash :: [String][se-String] SHA256 NAR hash base 16
- references :: [List][se-List] of [StorePath][se-StorePath]
- registrationTime :: [Time][se-Time]
- narSize :: [UInt64][se-UInt64]
- ultimate :: [Bool64][se-Bool64]
- signatures :: [List][se-List] of [String][se-String]
- ca :: [OptContentAddress][se-OptContentAddress]
- repair :: [Bool64][se-Bool64]
- dontCheckSigs :: [Bool64][se-Bool64]

#### If protocol version is 1.23 or newer
[Framed][se-Framed] NAR dump

#### If protocol version is between 1.21 and 1.23
NAR dump sent using `STDERR_READ`

#### If protocol version is older than 1.21
NAR dump sent raw on stream


## QueryMissing

**Id:** 40<br>
**Introduced:** Protocol 1.19*, Nix 2.0<br>

### Inputs
targets :: [List][se-List] of [DerivedPath][se-DerivedPath]

### Outputs
- willBuild :: [List][se-List] of [StorePath][se-StorePath]
- willSubstitute :: [List][se-List] of [StorePath][se-StorePath]
- unknown :: [List][se-List] of [StorePath][se-StorePath]
- downloadSize :: [UInt64][se-UInt64]
- narSize :: [UInt64][se-UInt64]


## QueryDerivationOutputMap

**Id:** 41<br>
**Introduced:** Protocol 1.22*, Nix 2.4<br>

Retrieves an associative map outputName -> storePath for a given derivation.

### Inputs
path :: [StorePath][se-StorePath]  (must be a derivation path)

### Outputs
outputs :: [Map][se-Map] of [String][se-String] to [OptStorePath][se-OptStorePath]


## RegisterDrvOutput

**Id:** 42<br>
**Introduced:** Protocol 1.27, Nix 2.4<br>

Registers a DRV output

### Inputs
#### If protocol is 1.31 or newer
realisation :: [Realisation][se-Realisation]

#### If protocol is older than 1.31
- outputId :: [DrvOutput][se-DrvOutput]
- outputPath :: [StorePath][se-StorePath]


## QueryRealisation

**Id:** 43<br>
**Introduced:** Protocol 1.27, Nix 2.4<br>

Retrieves the realisations attached to a drv output id realisations.

### Inputs
outputId :: [DrvOutput][se-DrvOutput]

### Outputs
#### If protocol is 1.31 or newer
realisations :: [List][se-List] of [Realisation][se-Realisation]

#### If protocol is older than 1.31
outPaths :: [List][se-List] of [BaseStorePath][se-BaseStorePath]


## AddMultipleToStore

**Id:** 44<br>
**Introduced:** Protocol 1.32*, Nix 2.4<br>

A pipelined version of [AddToStoreNar](#addtostorenar) where you can add
multiple paths in one go.

Added because the protocol doesn't support pipelining and so on a low latency
connection waiting for the request/response of [AddToStoreNar](#addtostorenar)
for each small NAR was costly.

### Inputs
- repair :: [Bool64][se-Bool64]
- dontCheckSigs :: [Bool64][se-Bool64]
- [Framed][se-Framed] stream of add multiple NAR dump


## AddBuildLog

**Id:** 45<br>
**Introduced:** Protocol 1.32, Nix 2.6.0<br>

Attach some build logs to a given build.

### Inputs
- path :: [String][se-String] (might be [BaseStorePath][se-BaseStorePath])
- [Framed][se-Framed] stream of log lines

### Outputs
1 :: [Int][se-Int] (hardcoded)


## BuildPathsWithResults

**Id:** 46<br>
**Introduced:** Protocol 1.34*, Nix 2.8.0<br>

Build (or substitute) a list of derivations and returns a list of results.

### Inputs
- drvs :: [List][se-List] of [DerivedPath][se-DerivedPath]
- mode :: [BuildMode][se-BuildMode]

### Outputs
results :: [List][se-List] of [KeyedBuildResult][se-KeyedBuildResult]


## AddPermRoot

**Id:** 47<br>
**Introduced:** Protocol 1.36*, Nix 2.20.0<br>

### Inputs
- storePath :: [StorePath][se-StorePath]
- gcRoot :: [String][se-String]

### Outputs
gcRoot :: [String][se-String]



[se-Int]: ./serialization.md#int
[se-UInt8]: ./serialization.md#uint8
[se-UInt64]: ./serialization.md#uint64
[se-Bool]: ./serialization.md#bool
[se-Bool64]: ./serialization.md#bool64
[se-Time]: ./serialization.md#time
[se-FileIngestionMethod]: ./serialization.md#fileingestionmethod
[se-BuildMode]: ./serialization.md#buildmode
[se-Verbosity]: ./serialization.md#verbosity
[se-GCAction]: ./serialization.md#gcaction
[se-Bytes]: ./serialization.md#bytes
[se-String]: ./serialization.md#string
[se-StorePath]: ./serialization.md#storepath
[se-BaseStorePath]: ./serialization.md#basestorepath
[se-OptStorePath]: ./serialization.md#optstorepath
[se-ContentAddressMethodWithAlgo]: ./serialization.md#contentaddressmethodwithalgo
[se-OptContentAddress]: ./serialization.md#optcontentaddress
[se-DerivedPath]: ./serialization.md#derivedpath
[se-DrvOutput]: ./serialization.md#drvoutput
[se-Realisation]: ./serialization.md#realisation
[se-List]: ./serialization.md#list-of-x
[se-Map]: ./serialization.md#map-of-x-to-y
[se-SubstitutablePathInfo]: ./serialization.md#substitutablepathinfo
[se-ValidPathInfo]: ./serialization.md#validpathinfo
[se-UnkeyedValidPathInfo]: ./serialization.md#unkeyedvalidpathinfo
[se-BuildResult]: ./serialization.md#buildmode
[se-KeyedBuildResult]: ./serialization.md#keyedbuildresult
[se-BasicDerivation]: ./serialization.md#basicderivation
[se-Framed]: ./serialization.md#framed