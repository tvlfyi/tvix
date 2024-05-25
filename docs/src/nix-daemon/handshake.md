

## client -> server
- 0x6e697863 :: [Int](#int) (hardcoded, 'nixc' in ASCII)

## server -> client
- 0x6478696f :: [Int](#int) (hardcoded, 'dxio' in ASCII)
- protocolVersion :: [Int](#int)

## client -> server
- clientVersion :: [Int](#int)

### If clientVersion is 1.14 or later
- sendCpu :: [Bool](#bool) (hardcoded to false in client)
#### If sendCpu is true
- cpuAffinity :: [Int](#int) (obsolete and ignored)

### If clientVersion is 1.11 or later
- reserveSpace :: [Bool](#bool) (obsolete, ignored and set to false)


## server -> client

### If clientVersion is 1.33 or later
- nixVersion :: String

### If clientVersion is 1.35 or later
- trusted :: OptTrusted

## server -> client
- send logs
- [operation](./operations.md) :: Int