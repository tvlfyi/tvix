
### UInt64
Little endian byte order

### Bytes

- len :: [UInt64](#uint64)
- len bytes of content
- padding with zeros to ensure 64 bit alignment of content + padding


## Int serializers

### Int
[UInt64](#uint64) cast to C `unsigned int` with upper bounds checking.

### Int64
[UInt64](#uint64) cast to C `int64_t` with upper bounds checking.
This means that negative numbers can be written but not read.
Since this is only used for cpuSystem and cpuUser it is fine that
negative numbers aren't supported.

### UInt8
[UInt64](#uint64) cast to C `uint8_t` with upper bounds checking.

### Size
[UInt64](#uint64) cast to C `size_t` with upper bounds checking.

### Time
[UInt64](#uint64) cast to C `time_t` with upper bounds checking.
This means that negative numbers can be written but not read.

### Bool
Sent as an [Int](#int) where 0 is false and everything else is true.

### Bool64
Sent as an [UInt64](#uint64) where 0 is false and everything else is true.

### FileIngestionMethod
An [UInt8](#uint8) enum with the following possible values:

| Name       | Int |
| ---------- | --- |
| Flat       |  0  |
| NixArchive |  1  |

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

### OptTrusted
An [UInt8](#uint8) optional enum with the following possible values:

| Name             | Int |
| ---------------- | --- |
| None             |  0  |
| Some(Trusted)    |  1  |
| Some(NotTrusted) |  2  |


## Bytes serializers

### String
Simply a [Bytes](#bytes) that has some UTF-8 string like semantics sometimes.

### StorePath
[String](#string) representation of a full store path including the store directory.

### BaseStorePath
[String](#string) representation of the basename of a store path. That is the store path
without the /nix/store prefix.

### StorePathName
[String](#string) representation of the name part of a base store path. This is the part
of the store path after the nixbase32 hash and '-'

It must have the following format:
- Deny ".", "..", or those strings followed by '-'
- Otherwise check that each character is 0-9, a-z, A-Z or one of +-._?=

### StorePathHash
[String](#string) representation of the hash part of a base store path. This is the part
of the store path at the beginning and before the '-' and is in nixbase32 format.


### OutputName
[String](#string) representation of the name of a derivation output.
This is usually combined with the name in the derivation to form the store path name for the
store path with this output.

Since output name is usually combined to form a store path name its format must follow the
same rules as [StorePathName](#storepathname):
- Deny ".", "..", or those strings followed by '-'
- Otherwise check that each character is 0-9, a-z, A-Z or one of +-._?=


### OptStorePath
Optional store path.

If no store path this is serialized as the empty string otherwise it is the same as
[StorePath](#storepath).

### Path
[String](#string) representation of an absolute path.

### NARHash
[String](#string) base16-encoded NAR SHA256 hash without algorithm prefix.

### Signature
[String](#string) with a signature for the given store path or realisation. This should be
in the format `name`:`base 64 encoded signature` but this is not enforced in the protocol.

### HashAlgorithm
[String](#string) with one of the following values:
- md5
- sha1
- sha256
- sha512

### HashDigest
[String](#string) with a hash digest in any encoding

### OptHashDigest
Optional version of [HashDigest](#hashdigest) where empty string means
no value.


### ContentAddressMethodWithAlgo
[String](#string) with one of the following formats:
- text:[HashAlgorithm](#hashalgorithm)
- fixed:r:[HashAlgorithm](#hashalgorithm)
- fixed:[HashAlgorithm](#hashalgorithm)

### OptContentAddressMethodWithAlgo
Optional version of [ContentAddressMethodWithAlgo](#contentaddressmethodwithalgo)
where empty string means no value.

### ContentAddress
[String](#string) with the format:
- [ContentAddressMethodWithAlgo](#contentaddressmethodwithalgo):[HashDigest](#hashdigest)

### OptContentAddress
Optional version of [ContentAddress](#contentaddress) where empty string means
no content address.

### DerivedPath
#### If protocol is 1.30 or newer
output-names = [OutputName](#outputname), { "," [OutputName](#outputname) }<br>
output-spec = "*" | output-names<br>
derived-path = [StorePath](#storepath), [ "!", output-spec ]<br>

#### If protocol is older than 1.30
[StorePath](#storepath), [ "!", [OutputName](#outputname), { "," [OutputName](#outputname) } ]

### DrvOutput
[String](#string) with format:
- `hash with any prefix` "!" [OutputName](#outputname)

### Realisation
A JSON object sent as a [String](#string).

The JSON object has the following keys:
| Key                   | Value                            |
| --------------------- | -------------------------------- |
| id                    | [DrvOutput](#drvoutput)          |
| outPath               | [StorePath](#storepath)          |
| signatures            | Array of [Signature](#signature) |
| dependentRealisations | Object where key is [DrvOutput](#drvoutput) and value is [StorePath](#storepath) |


## Complex serializers

### List of x
A list is encoded as a [Size](#size) length n followed by n encodings of x

### Map of x to y
A map is encoded as a [Size](#size) length n followed by n encodings of pairs of x and y

### Set of x
A set is encoded as a [Size](#size) length n followed by n encodings of x

### BuildResult
- status :: [BuildStatus](#buildstatus)
- errorMsg :: [String](#string)

#### Protocol 1.29 or newer
- timesBuilt :: [Int](#int) (defaults to 0)
- isNonDeterministic :: [Bool64](#bool64) (defaults to false)
- startTime :: [Time](#time) (defaults to 0)
- stopTime :: [Time](#time) (defaults to 0)

#### Protocol 1.37 or newer
- cpuUser :: [OptMicroseconds](#optmicroseconds) (defaults to none)
- cpuSystem :: [OptMicroseconds](#optmicroseconds) (defaults to none)

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
- deriver :: [OptStorePath](#optstorepath)
- references :: [Set][#set-of-x] of [StorePath](#storepath)
- downloadSize :: [UInt64](#uint64)
- narSize :: [UInt64](#uint64)


### UnkeyedValidPathInfo
- deriver :: [OptStorePath](#optstorepath)
- narHash :: [NARHash](#narhash)
- references :: [Set](#set-of-x) of [StorePath](#storepath)
- registrationTime :: [Time](#time)
- narSize :: [UInt64](#uint64)

#### If protocol version is 1.16 or above
- ultimate :: [Bool64](#bool64) (defaults to false)
- signatures :: [Set](#set-of-x) of [Signature](#signature)
- ca :: [OptContentAddress](#optcontentaddress)


### ValidPathInfo
- path :: [StorePath](#storepath)
- info :: [UnkeyedValidPathInfo](#unkeyedvalidpathinfo)

### DerivationOutput
- path :: [OptStorePath](#optstorepath)
- hashAlgo :: [OptContentAddressMethodWithAlgo](#optcontentaddressmethodwithalgo)
- hash :: [OptHashDigest](#opthashdigest)

### BasicDerivation
- outputs :: [Map](#map-of-x-to-y) of [OutputName](#outputname) to [DerivationOutput](#derivationoutput)
- inputSrcs :: [Set](#set-of-x) of [StorePath](#storepath)
- platform :: [String](#string)
- builder :: [String](#string)
- args :: [List](#list-of-x) of [String](#string)
- env :: [Map](#map-of-x-to-y) of [String](#string) to [String](#string)

### TraceLine
- havePos :: [Size](#size) (hardcoded to 0)
- hint :: [String](#string) (If logger is JSON, invalid UTF-8 is replaced with U+FFFD)

### Error
- type :: [String](#string) (hardcoded to `Error`)
- level :: [Verbosity](#verbosity)
- name :: [String](#string) (removed and hardcoded to `Error`)
- msg :: [String](#string) (If logger is JSON, invalid UTF-8 is replaced with U+FFFD)
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

At its core the framed streaming is just a series of [UInt64](#uint64) `size`
followed by `size` bytes. The stream is terminated when `size` is zero.

Unlike [Bytes](#bytes), frames are *NOT* padded.

This method of sending data has the advantage of not having to parse the data
to find where it ends. Older versions of the protocol would potentially parse
the NAR twice.


## AddMultipleToStore format

Paths must be topologically sorted.

- expected :: [UInt64](#uint64)

### Repeated expected times
- info :: [ValidPathInfo](#validpathinfo)
- NAR dump


## Export path format
- NAR dump
- 0x4558494es :: [Int](#int) (hardcoded, 'EXIN' in ASCII)
- path :: [StorePath](#storepath)
- references :: [Set](#set-of-x) of [StorePath](#storepath)
- deriver :: [OptStorePath](#optstorepath)
- hasSignature :: [Int](#int) (hardcoded to 0 in newer versions)

#### If hasSignature is 1
- signature :: [String](#string) (ignored)


## Import paths format

- hasNext :: [UInt64](#uint64)

### While hasNext is 1
- [Export path format](#export-path-format)
- hasNext :: [UInt64](#uint64)
