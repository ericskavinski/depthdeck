# Architecture

DepthDeck separates storage integrity from market-state integrity. A tape can be structurally intact while containing a bad venue checksum; both layers are verified and produce different diagnostics.

## Data path

1. `depthdeck capture` requests instrument precision before opening the output tape.
2. The capture loop subscribes to Kraken's `book` channel, records connection lifecycle markers, and sends every snapshot or update through a live `ReplaySession` before writing it.
3. A checksum mismatch clears synchronization and forces a reconnect. Updates are rejected until a fresh valid snapshot arrives.
4. `TapeWriter` buffers at most one second or 1 MiB of records, compresses the chunk with Zstandard, and stores the uncompressed CRC32.
5. `TapeReader` validates the framing, chunk lengths, CRCs, record counts, time ranges, footer index, and final marker before exposing records.
6. `ReplaySession` parses Kraken-shaped records into scaled integers, applies each message to a candidate book, verifies the venue checksum, then commits the candidate atomically.

## Shared core

`depthdeck-core` has no UI dependency. Native builds use the `zstd` C bindings for writing and reading; WebAssembly uses the pure-Rust `ruzstd` decoder and intentionally exposes no writer. `depthdeck-wasm` is a thin serialization boundary around `ReplaySession`. The browser transfers a tape to a dedicated Worker, so decoding and replay do not block the UI thread.

## Determinism

- Receive offsets are monotonic integer nanoseconds.
- Prices and quantities are parsed into scaled `i64` atoms using precision captured in tape metadata.
- Bids and asks have explicit ordering and depth truncation.
- The state digest covers message and mutation counts plus ordered price/quantity atoms.
- The demo generator has no clock or random-number dependency.

## Deliberate constraints

- Kraken Spot WebSocket v2 only.
- L2 book data only.
- One symbol per tape.
- Replay currently reconstructs from the start when seeking backward. For the 90-second public tape this remains interactive; persisted replay checkpoints are the natural extension for multi-hour captures.
- The format is versioned but v1 has no schema-migration machinery yet. Unknown versions fail closed.

These constraints keep exchange semantics visible and make failure behavior auditable instead of hiding it behind a broad market-data interface.

