# ADR 0016: Download storage — one copy, finished files in the user's folders

**Status:** proposed (implemented after the Phase 3–7 review; owner asked for "minimum SSD wear, maximum speed and durability")

## Problem
The Phase 5 build wrote most downloads twice or more:
- **Torrent:** librqbit stored the file, and a copy loop duplicated it into a BlobStore blob (2× disk, 2× writes).
- **yt-dlp:** tracks were written to a temp folder, (muxed,) then copied into a blob (2–3× writes). They were not resumable and could not play while downloading.
- **All:** finished files stayed as opaque `<hex>.blob` files in the data folder. The per-type folder settings were never used.

## Decision
1. **Torrent downloads.**
   - librqbit writes straight into `<folder>/.cinma-partial/<info hash>/`. That folder belongs to Cinma, so `overwrite` is safe there; it is never a user path.
   - When finished, the file is moved to `<folder>/<file name>` by a hard link plus unlink: no data is copied, and an existing file is never replaced.
   - Pause detaches the torrent from the session (files kept). Resume re-adds it, and librqbit re-checks the pieces already on disk.
   - "Play Now" streams from the same torrent.
2. **HTTP downloads.**
   - Bytes go into a blob as before: sparse, resumable, playable while downloading.
   - On completion the blob's data file is hard-linked to `<folder>/<title>.<ext>` and the blob is forgotten. Across volumes it is copied to `.part`, fsynced and renamed.
3. **yt-dlp.**
   - A single track is handed to the HTTP engine: one write, resumable, playable, then moved like an HTTP download. Resume ignores URL changes, because signed URLs differ on every probe; it still checks size and ETag/Last-Modified.
   - Separate video and audio tracks still need temp parts plus a mux, written into the folder's `.cinma-partial`, then moved.
4. **Folders.**
   - Video (movies, series, web video): `~/Movies/Cinma`.
   - Audio-only: `~/Music/Cinma`.
   - Both can be changed in Settings.
5. **Crash safety.** The destination path is stored in `download.file_path` (migration 0007) **before** the move, and the blob reference is cleared after it. Re-running any step after a crash is safe. Completed rows that still have a blob are finished at queue start-up.
6. **Deletion.** Only exact paths are ever deleted: the blob, `staging_dir(folder, hash)` and the recorded `file_path`. The UI never deletes a finished file directly: it offers "Remove from List" (file kept) or "Move File to Trash".
7. **SSD.**
   - Download checkpoints (blob fsync, sidecar, SQLite) run every 10 s instead of 2 s.
   - Torrent progress is written every 5 s, only while a download is active.

## Consequences
- **The disk quota and "keep watched" have nothing left to manage.** Streamed data is deleted when playback stops, and finished downloads are the user's files, not cache. Their settings are hidden; the code remains for a future streaming cache.
- **Streaming still writes pieces to disk** (librqbit storage, ADR 0009). Streaming from RAM only would be an engine change and is left to the owner.
