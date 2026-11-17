# ADR 0011 — Phase 4: Seek-bar thumbnails, dedicated GL rendering, subtitle pipeline, and PiP spike

**Status:** accepted (owner, 2026-11-17; PiP updated per owner decision §4)

Phase 4 completes playback (`CLAUDE.md` §5.2, §5.8, §5.9; `docs/ROADMAP.md` §7) while keeping the launch (< 250 ms) and idle (<= 40 MB) budgets intact.

## 1. Seek-bar keyframe thumbnails
- **Runtime `dlopen`, zero build-time dylib linkage (ADR 0005 pattern):**
  - Look up Homebrew FFmpeg shared libraries in `CINMA_FFMPEG_LIB` → `/opt/homebrew/opt/ffmpeg/lib` → `/opt/homebrew/lib` → `/usr/local/lib`: `libavformat.63.dylib`, `libavcodec.63.dylib`, `libswscale.10.dylib`, `libavutil.61.dylib`.
  - Verify runtime ABI major versions via `avformat_version() >> 16 == 63`, `avcodec_version() >> 16 == 63`, and `swscale_version() >> 16 == 10` before calling any struct-offset accessors; refuse cleanly and fall back to `AVAssetImageGenerator` (AVFoundation) for local files if FFmpeg is missing or a different major ABI is installed.
- **Strictly non-blocking on streams:**
  - `ByteSource` gains `try_read_at(offset, buf) -> Option<usize>`, which reads **only** from already-downloaded byte ranges (`BlobSource` checks `RangeSet`, `TorrentSource` reads tracked downloaded ranges from its on-disk file without touching `FileStream::seek`, and local files read directly).
  - The FFmpeg `AVIOContext` wraps `try_read_at` and returns `AVERROR_EOF` immediately on uncached byte holes, so hovering the seek bar **never** triggers BitTorrent piece priority changes or HTTP range fetches.
- **Output & memory budget:**
  - Decode nearest keyframe (`AVSEEK_FLAG_BACKWARD` + `AVDISCARD_NONKEY`), scale to ~160 px wide RGBA via `sws_scale`, and cache in a bounded in-memory LRU (~8 MB max) that is dropped immediately when playback ends.

## 2. Dedicated GL render thread & SDR framebuffer path
- **Render thread:** Switch `VideoLayer` (`CAOpenGLLayer`) to `isAsynchronous = true` paired with `MPV_RENDER_PARAM_ADVANCED_CONTROL = 1`, `mpv_render_context_update()` inside `canDraw(inCGLContext:)` (`MPV_RENDER_UPDATE_FRAME`), and `MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME = 0` inside `draw(inCGLContext:)`, serializing CGL context access with `CGLLockContext` / `CGLUnlockContext`. This moves libmpv GL draw calls off the AppKit main thread onto Core Animation's background `CVDisplayLink` thread while preserving 120 Hz scrub-bar hover responsiveness.
- **SDR 8-bit surface optimization:** When the selected stream/file is known to be SDR (no HDR/DV tags from ranker or local file inspection) and the user has not forced HDR output, initialize `VideoLayer` with an 8-bit `ITUR_709` surface (`wantsExtendedDynamicRangeContent = false`) and configure mpv with `fbo-format=rgba8`, `target-trc=bt.1886`, `target-prim=bt.709` — avoiding the 16-bit float framebuffer cost and unintended EDR engagement on SDR content. HDR10/HLG/Dolby Vision streams still initialize the PQ/BT.2020 16-bit float EDR surface per ADR 0004.
- **Playback memory trim:** Trim demuxer defaults to `demuxer-max-bytes=16MiB` and `demuxer-max-back-bytes=4MiB` and enable `vd-lavc-dr=yes` under advanced render control.

## 3. Subtitles, OpenSubtitles hash, and Turkish encoding
- **OpenSubtitles hash:** Standard 64-bit little-endian sum over first 64 KiB + last 64 KiB + byte size (`%016llx`). Computed automatically once both 64 KiB ranges arrive (`librqbit` warmup fetches both head and tail) via `ByteSource::try_read_at`, or immediately for local files.
- **Providers:** Query enabled Stremio addons exposing the `subtitles` resource (`videoHash`, `videoSize`, `filename`) plus optional direct OpenSubtitles REST API v1 (API key & user token stored in macOS Keychain under `app.cinma.mac`: `opensubtitles.api_key`, `opensubtitles.token`).
- **Encoding & styling:** Inspect subtitle bytes for UTF-8 BOM, UTF-16 LE/BE BOM, valid UTF-8, or legacy single-byte encodings using `encoding_rs` — explicitly detecting Turkish `windows-1254` / `ISO-8859-9` and converting SRT/VTT/SUB to cached UTF-8 files under `data_dir/subtitles/`. ASS/SSA files keep their styling untouched unless the user explicitly enables `sub-ass-override` in settings.
- **Profile preferences:** Stored in existing `profile_setting` SQLite table (`0002_schema.sql`), including language priority lists, dual-subtitle second language, SRT style overrides, custom font, and per-video `sub_delay` / `audio_delay` offsets.

## 4. PiP decision (updated 2026-11-17, owner)
- `AVPictureInPictureController` on macOS requires either an `AVPlayerLayer` or an `AVPictureInPictureController.ContentSource(sampleBufferDisplayLayer:playbackDelegate:)` backed by `AVSampleBufferDisplayLayer`, which would add a per-frame `CVPixelBuffer` → `CMSampleBuffer` copy pipeline over our OpenGL `CAOpenGLLayer` (`ADR 0004`).
- **Owner decision:** Implement PiP as an AppKit always-on-top borderless mini-player `NSPanel` reusing `VideoLayer` with zero frame copy (preserving HDR and subtitles with no extra CPU cost), scheduled in Phase 8 or Phase 9. Do not build the frame-copy system PiP path.
