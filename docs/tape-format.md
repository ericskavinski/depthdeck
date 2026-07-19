# DepthDeck tape format (`.ddt`) — version 1

All integers are unsigned little-endian unless stated otherwise. Byte strings are stored without terminators.

## File layout

```text
header
chunk 0
chunk 1
...
footer index
end marker
```

### Header

| Field | Size | Value |
| --- | ---: | --- |
| magic | 8 | ASCII `DPTHDCK1` |
| version | 2 | `1` |
| metadata length | 4 | JSON byte length |
| metadata | variable | UTF-8 JSON `TapeMetadata` |

Metadata records the exchange identifier, symbol, requested depth, price and quantity precision, capture start as Unix nanoseconds, and generator version.

### Chunk

| Field | Size | Description |
| --- | ---: | --- |
| marker | 4 | ASCII `CHNK` |
| first receive offset | 8 | first record's nanosecond offset |
| last receive offset | 8 | last record's nanosecond offset |
| record count | 4 | records in uncompressed payload |
| uncompressed length | 4 | bytes before compression |
| compressed length | 4 | following Zstandard bytes |
| CRC32 | 4 | CRC32 of the uncompressed payload |
| payload | variable | Zstandard frame, level 3 |

Writers close a chunk when its receive-time span reaches one second or its uncompressed body would exceed 1 MiB.

Each uncompressed record is:

| Field | Size | Description |
| --- | ---: | --- |
| receive offset | 8 | monotonic nanoseconds from capture start |
| kind | 1 | `1` opened, `2` lost, `3` snapshot, `4` update, `5` checksum mismatch |
| payload length | 4 | raw payload bytes |
| payload | variable | Kraken JSON or marker JSON |

### Footer

| Field | Size | Description |
| --- | ---: | --- |
| marker | 4 | ASCII `INDX` |
| index length | 4 | following JSON byte length |
| index | variable | JSON array of first/last offsets and chunk file offsets |
| end marker | 4 | ASCII `END!` |

Readers fail closed on bad magic, unsupported versions, truncation, unknown record kinds, non-monotonic records, chunk range/count mismatches, CRC failures, footer inconsistencies, trailing bytes, or a missing end marker. Integrity errors identify the relevant byte offset where possible.

