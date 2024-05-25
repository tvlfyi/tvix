# Logging

Because the daemon protocol only has one sender stream and one receiver stream
logging messages need to be carefully interleaved with requests and responses.
Usually this means that after the operation and all of its inputs (the request)
has been read logging hijacks the sender stream (in the server case) and uses
it to send typed logging messages while the request is being processed. When
the response has been generated it will send `STDERR_LAST` to mark that what
follows is the response data to the request. If the request failed a
`STDERR_ERROR` message is sent with the error and no response is sent.

While not in this state between request reading and response sending all
messages and activities are buffered until next time the logger can send data.

The logging messages supported are:
- [`STDERR_LAST`](#stderr_last)
- [`STDERR_ERROR`](#stderr_error)
- [`STDERR_NEXT`](#stderr_next)
- [`STDERR_READ`](#stderr_read)
- [`STDERR_WRITE`](#stderr_write)
- [`STDERR_START_ACTIVITY`](#stderr_start_activity)
- [`STDERR_STOP_ACTIVITY`](#stderr_stop_activity)
- [`STDERR_RESULT`](#stderr_result)


### `STDERR_LAST`
Marks the end of the logs, normal processing can resume.

- 0x616c7473 :: [UInt64][se-UInt64] (hardcoded, 'alts' in ASCII)


### `STDERR_ERROR`
This also marks the end of this log "session" and so it
has the same effect as `STDERR_LAST`.
On the client the error is thrown as an exception and no response is read.

#### If protocol version is 1.26 or newer
- 0x63787470 :: [UInt64][se-UInt64] (hardcoded, 'cxtp' in ASCII)
- error :: [Error][se-Error]

#### If protocol version is older than 1.26
- 0x63787470 :: [UInt64][se-UInt64] (hardcoded, 'cxtp' in ASCII)
- msg :: [String][se-String] (If logger is JSON, invalid UTF-8 is replaced with U+FFFD)
- exitStatus :: [Int][se-Int]


### `STDERR_NEXT`
Normal string log message.

- 0x6f6c6d67 :: [UInt64][se-UInt64] (hardcoded, 'olmg' in ASCII)
- msg :: [String][se-String] (If logger is JSON, invalid UTF-8 is replaced with U+FFFD)


### `STDERR_READ`
Reader interface used by ImportsPaths and AddToStoreNar (between 1.21 and 1.23).
It works by sending a desired buffer length and then on the receiver stream it
reads bytes buffer of that length. If it receives 0 bytes it sees this as an
unexpected EOF.

- 0x64617461 :: [UInt64][se-UInt64] (hardcoded, 'data' in ASCII)
- desiredLen :: [Size][se-Size]


### `STDERR_WRITE`
Writer interface used by ExportPath. Simply writes a buffer.

- 0x64617416 :: [UInt64][se-UInt64] (hardcoded)
- buffer :: [Bytes][se-Bytes]


### `STDERR_START_ACTIVITY`
Begins an activity. In other tracing frameworks this would be called a span.

Implemented in protocol 1.20. To achieve backwards compatible with older
versions of the protocol instead of sending an `STDERR_START_ACTIVITY`
the level is checked against enabled logging level and the text field is
sent as a simple log message with `STDERR_NEXT`.

- 0x53545254 :: [UInt64][se-UInt64] (hardcoded, 'STRT' in ASCII)
- act :: [UInt64][se-UInt64]
- level :: [Verbosity][se-Verbosity]
- type :: [ActivityType][se-ActivityType]
- text :: [String][se-String] (If logger is JSON, invalid UTF-8 is replaced with U+FFFD)
- fields :: [List][se-List] of [Field][se-Field]
- parent :: [UInt64][se-UInt64]


act is atomic (nextId++ + (getPid() << 32))


### `STDERR_STOP_ACTIVITY`
Stops the given activity. The activity id should not send any more results.
Just sends `ActivityId`.

Implemented in protocol 1.20. When backwards compatible with older versions of
the protocol and this message would have been sent it is instead ignored.

- 0x53544f50 :: [UInt64][se-UInt64] (hardcoded, 'STOP' in ASCII)


### `STDERR_RESULT`
Sends results for a given activity.

Implemented in protocol 1.20. When backwards compatible with older versions of
the protocol and this message would have been sent it is instead ignored.

- 0x52534c54 :: [UInt64][se-UInt64] (hardcoded, 'RSLT' in ASCII)
- act :: [UInt64][se-UInt64]
- type :: [ResultType][se-ResultType]
- fields :: [List][se-List] of [Field][se-Field]



[se-UInt64]: ./serialization.md#uint64
[se-Int]: ./serialization.md#int
[se-Size]: ./serialization.md#size
[se-Verbosity]: ./serialization.md#verbosity
[se-ActivityType]: ./serialization.md#activitytype
[se-ResultType]: ./serialization.md#resulttype
[se-Bytes]: ./serialization.md#bytes
[se-String]: ./serialization.md#string
[se-List]: ./serialization.md#list-of-x
[se-Error]: ./serialization.md#error
[se-Field]: ./serialization.md#field