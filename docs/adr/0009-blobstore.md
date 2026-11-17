# ADR 0009 — BlobStore design, and torrents outside it (for now)

**Status:** accepted (2026-11-17; the owner delegated the call), including the deviation from §5.1/§1.6 for torrents described below.

## BlobStore
- A blob is a sparse data file (`<id>.blob`) plus a sidecar (`<id>.ranges`, JSON) listing the byte ranges that may be trusted.
- SQLite holds what a blob *is* (download rows reference `blob_id`). The sidecar only holds what's needed to trust the bytes.
- Writing and exposing are separate steps: `write_at` writes, `mark_available` exposes. An engine that verifies data (hashes) can write first and expose later.
- `checkpoint()` does this, in order:
  1. snapshot the marked ranges;
  2. `fsync` the data file;
  3. write the sidecar to a temp file, `fsync` it, `rename` it, then `fsync` the directory.
- The queue calls `checkpoint()` **before** writing `bytes_done` to SQLite, so a row never claims more than the blob can prove.
- Readers block on missing ranges. They wake on data or cancellation through `tokio::sync::Notify` plus `CancellationToken`, driven by a tiny thread-park executor, so there is no polling. Every read sends a `ReadHint` (offset, length, read-ahead) to the engine's `PriorityListener`.
- `fetch_order()` plans fetching as: head (4 MiB), tail (4 MiB), forward from the cursor, then the rest.
- Composable sources: `ByteSource` can wrap another `ByteSource`. That's the slot for store-mode RAR/ZIP; not implemented yet.

## Crash safety: what is and isn't proven
`kill -9` tests run in real child processes, killed at several points. They show:
- after a kill, every range the sidecar claims holds the right bytes;
- a reported checkpoint is never lost;
- a download row never claims more than its blob;
- SQLite passes `integrity_check`.

A killed process keeps its page cache, so these tests **don't** prove power-loss durability. That relies on `fsync` and the disk honouring it.

## Torrents stay on librqbit's own storage
librqbit tracks verified pieces internally (`chunk_tracker`, `piece_tracker` are crate-private) and exposes no "piece verified" hook. Its `TorrentStorage` trait only sees raw, unverified writes. Routing torrent bytes through our Blob would mean one of:

- (a) exposing unverified data;
- (b) re-hashing pieces ourselves;
- (c) patching librqbit.

Meanwhile librqbit's `FileStream` already provides verified, prioritised, blocking reads, and it is what the player uses through `ByteSource`. So torrents keep librqbit storage, and BlobStore serves HTTP and yt-dlp (Phase 5).

Disk policy (Phase 5) then has to handle two stores: blobs and librqbit download folders. If that becomes painful, revisit with (c), a small upstream PR adding a piece-completed callback, or (b).
