
### UInt64
Little endian byte order

### Bytes

- len :: [UInt64](#uint64)
- len bytes of content
- padding with zeros to ensure 64 bit alignment of content with padding


## Int serializers

### Int
[UInt64](#uint64) cast to C `unsigned int` with upper bounds checking.

### Int64
[UInt64](#uint64) cast to C `int64_t` with upper bounds checking.

### UInt8
[UInt64](#uint64) cast to C `uint8_t` with upper bounds checking.

### Size
[UInt64](#uint64) cast to C `size_t` with upper bounds checking.

### Time
[UInt64](#uint64) cast to C `time_t` with upper bounds checking.
s
### Bool
Sent as an [Int](#int) where 0 is false and everything else is true.

### Bool64
Sent as an [UInt64](#uint64) where 0 is false and everything else is true.

### FileIngestionMethod
An [UInt8](#uint8) enum with the following possible values:

| Name      | Int |
| --------- | --- |
| Flat      |  0  |
| Recursive |  1  |

### BuildMode
An [Int](#int) enum with the following possible values:

| Name   | Int |
| ------ | --- |
| Normal |  0  |
| Repair |  1  |
| Check  |  2  |

### Verbosity
An [Int](#int) enum with the following possible values:

| Name      | Int |
| --------- | --- |
| Error     |  0  |
| Warn      |  1  |
| Notice    |  2  |
| Info      |  3  |
| Talkative |  4  |
| Chatty    |  5  |
| Debug     |  6  |
| Vomit     |  7  |

### GCAction
An [Int](#int) enum with the following possible values:

| Name           | Int |
| -------------- | --- |
| ReturnLive     |  0  |
| ReturnDead     |  1  |
| DeleteDead     |  2  |
| DeleteSpecific |  3  |

### BuildStatus
An [Int](#int) enum with the following possible values:

| Name                   | Int |
| ---------------------- | --- |
| Built                  |  0  |
| Substituted            |  1  |
| AlreadyValid           |  2  |
| PermanentFailure       |  3  |
| InputRejected          |  4  |
| OutputRejected         |  5  |
| TransientFailure       |  6  |
| CachedFailure          |  7  |
| TimedOut               |  8  |
| MiscFailure            |  9  |
| DependencyFailed       | 10  |
| LogLimitExceeded       | 11  |
| NotDeterministic       | 12  |
| ResolvesToAlreadyValid | 13  |
| NoSubstituters         | 14  |

### ActivityType
An [Int](#int) enum with the following possible values:

| Name          | Int |
| ------------- | --- |
| Unknown       |   0 |
| CopyPath      | 100 |
| FileTransfer  | 101 |
| Realise       | 102 |
| CopyPaths     | 103 |
| Builds        | 104 |
| Build         | 105 |
| OptimiseStore | 106 |
| VerifyPaths   | 107 |
| Substitute    | 108 |
| QueryPathInfo | 109 |
| PostBuildHook | 110 |
| BuildWaiting  | 111 |
| FetchTree     | 112 |

### ResultType
An [Int](#int) enum with the following possible values:

| Name             | Int |
| ---------------- | --- |
| FileLinked       | 100 |
| BuildLogLine     | 101 |
| UntrustedPath    | 102 |
| CorruptedPath    | 103 |
| SetPhase         | 104 |
| Progress         | 105 |
| SetExpected      | 106 |
| PostBuildLogLine | 107 |
| FetchStatus      | 108 |

### FieldType
An [Int](#int) enum with the following possible values:

| Name   | Int |
| ------ | --- |
| Int    |  0  |
| String |  1  |


## Bytes serializers

### String
Simply a [Bytes](#bytes) that has some UTF-8 string like semantics sometimes.

### StorePath
String representation of a full store path.

### BaseStorePath
String representation of the basename of a store path. That is the store path
without the /nix/store prefix.

### OptStorePath
Optional store path.

If no store path this is serialized as the empty string otherwise it is the same as
[StorePath](#storepath).

### ContentAddressMethodWithAlgo
One of the following strings:
- text:`hash algorithm`
- fixed:r:`hash algorithm`
- fixed:`hash algorithm`

### DerivedPath
#### If protocol is 1.30 or newer
        return DerivedPath::parseLegacy(store, s);
#### If protocol is older than 1.30
        return parsePathWithOutputs(store, s).toDerivedPath();

### ContentAddress
String with the format:
- [ContentAddressMethodWithAlgo](#contentaddressmethodwithalgo):`hash`

### OptContentAddress
Optional version of [ContentAddress](#contentaddress) where empty string means
no content address.

### DrvOutput
String with format:
- `hash with any prefix`!`output name`

### Realisation
A JSON object sent as a string.

The JSON object has the following keys:
| Key                   | Value                   |
| --------------------- | ----------------------- |
| id                    | [DrvOutput](#drvoutput) |
| outPath               | [StorePath](#storepath) |
| signatures            | Array of String         |
| dependentRealisations | Object where key is [DrvOutput](#drvoutput) and value is [StorePath](#storepath) |


## Complex serializers

### List of x
A list is encoded as a [Size](#size) length n followed by n encodings of x

### Map of x to y
A map is encoded as a [Size](#size) length n followed by n encodings of pairs of x and y


### BuildResult
- status :: [BuildStatus](#buildstatus)
- errorMsg :: [String](#string)

#### Protocol 1.29 or newer
- timesBuilt :: [Int](#int)
- isNonDeterministic :: [Bool64](#bool64)
- startTime :: [Time](#time)
- stopTime :: [Time](#time)

#### Protocol 1.37 or newer
- cpuUser :: [OptMicroseconds](#optmicroseconds)
- cpuSystem :: [OptMicroseconds](#optmicroseconds)

#### Protocol 1.28 or newer
builtOutputs ::  [Map](#map-of-x-to-y) of [DrvOutput](#drvoutput) to [Realisation](#realisations)

### KeyedBuildResult
- path :: [DerivedPath](#derivedpath)
- result :: [BuildResult](#buildresult)

### OptMicroseconds
Optional microseconds.

- tag :: [UInt8](#uint8)

#### If tag is 1
- seconds :: [Int64](#int64)


### SubstitutablePathInfo
- storePath :: [StorePath](#storepath)
- deriver :: [OptStorePath](#optstorepath)
- references :: [List](#list-of-x) of [StorePath](#storepath)
- downloadSize :: [UInt64](#uint64)
- narSize :: [UInt64](#uint64)


### UnkeyedValidPathInfo
- deriver :: [OptStorePath](#optstorepath)
- narHash :: [String](#string) SHA256 NAR hash base16 encoded
- references :: [List](#list-of-x) of [StorePath](#storepath)
- registrationTime :: [Time](#time)
- narSize :: [UInt64](#uint64)

#### If protocol version is 1.16 or above
- ultimate :: [Bool64](#bool64)
- signatures :: [List](#list-of-x) of [String](#string)
- ca :: [OptContentAddress](#optcontentaddress)


### ValidPathInfo
- path :: [StorePath](#storepath)
- info :: [UnkeyedValidPathInfo](#unkeyedvalidpathinfo)

### DerivationOutput
- path :: [String](#string)
- hashAlgo :: [String](#string)
- hash :: [String](#string)

### BasicDerivation
- outputs :: [Map](#map-of-x-to-y) of [String](#string) to [DerivationOutput](#derivationoutput)
- inputSrcs :: [List](#list-of-x) of [StorePath](#storepath)
- platform :: [String](#string)
- builder :: [String](#string)
- args :: [List](#list-of-x) of [String](#string)
- env :: [Map](#map-of-x-to-y) of [String](#string) to [String](#string)

### TraceLine
- havePos :: [Size](#size) (hardcoded to 0)
- hint :: [String](#string)

### Error
- type :: [String](#string) (hardcoded to `Error`)
- level :: [Verbosity](#verbosity)
- name :: [String](#string) (removed and hardcoded to `Error`)
- msg :: [String](#string)
- havePos :: [Size](#size) (hardcoded to 0)
- traces :: [List](#list-of-x) of [TraceLine](#traceline)

## Field
- type :: [FieldType](#fieldtype)

### If type is Int
- value :: [UInt64](#uint64)

### If type is String
- value :: [String](#string)


## Framed

At protocol 1.23 [AddToStoreNar](./operations.md#addtostorenar) introduced a
framed streaming for sending the NAR dump and later versions of the protocol
also used this framing for other operations.

At its core the framed streaming is just a series of [Bytes](#bytes) of
varying length and terminated by an empty [Bytes](#bytes).

This method of sending data has the advantage of not having to parse the data
to find where it ends. Older versions of the protocol would potentially parse
the NAR twice.