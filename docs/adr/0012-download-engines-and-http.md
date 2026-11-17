# ADR 0012 — Unified download engines: Segmented HTTP/1.1, Torrent download mode, and yt-dlp in-process muxing

**Status:** proposed (implemented in Phase 5; awaiting owner review)

## 1. Segmented HTTP/1.1 engine (`crates/features/http`)
- **HTTP/1.1 multi-connection client:** Uses a dedicated `reqwest::Client` built with `.http1_only()` (`CLAUDE.md` §5.4 — HTTP/2 multiplexing defeats parallel range streams), per-host concurrency limit (default 4 connections/host), and clean cancellation via `CancellationToken`.
- **Resume & cache validation:** Probes `HEAD` (falling back to `GET Range: bytes=0-0`) for `Content-Length`, `Accept-Ranges: bytes`, `ETag`, and `Last-Modified`. Validator metadata persists in `.http.meta` alongside the `BlobStore` / `source_json`; if the validator changes between pause and resume, the download starts cleanly from 0 rather than stitching mismatched bytes.
- **Dynamic segmentation & live playback priority:**
  - Splits the largest remaining unclaimed gap across workers whenever a connection finishes.
  - Registers a `PriorityListener` on `job.blob(size)`: when playback starts via `Blob::source()` ("Play Now"), read hints notify the scheduler so a dedicated cursor worker immediately claims the read-ahead window at `hint.offset` without waiting for earlier chunks.
  - Checkpoints the blob before recording SQLite progress (`job.report_progress(&blob)`).

## 2. Torrent download mode (`cinma-torrent`)
- Reuses `RqbitEngine` with `SessionPersistenceConfig::Json` (`fastresume: true`) and configurable `LimitsConfig` (`upload_bps`, `download_bps`).
- Supports full completion, file selection in multi-file torrents (`file_idx`), rate limits, pause/resume, and seeding policies (`off` = immediate stop on completion; `ratio` / `time` = capped background seeding).

## 3. `yt-dlp` downloads & in-process muxing (`libavformat`)
- Invokes `yt-dlp -J --no-playlist <url>` as a short-lived child to inspect formats (supporting best video+audio muxed, capped resolution, or `audio_only`).
- Downloads media streams through the segmented HTTP engine (`crates/features/http`).
- When separate video + audio formats are selected, muxes in-process via our already `dlopen`ed Homebrew `libavformat.63.dylib` (`avformat_alloc_output_context2`, `avformat_new_stream`, `avcodec_parameters_copy`, `av_interleaved_write_frame`) directly to disk/BlobStore — no `ffmpeg` subprocess spawned.
