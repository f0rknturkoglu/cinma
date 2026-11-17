# PROGRESS

## Status: Phases 0–7 complete (end-to-end roadmap built & verified; risks documented in `docs/RISKS.md`).

## Phases 5, 6 & 7 — Unified Downloads, Integrations, Security & Packaging ✅

### Done in Phase 5 (Unified Download Manager, Disk Policy & Menu Bar)
- **ADRs & Migrations**: `docs/adr/0012-download-engines-and-http.md`, `docs/adr/0013-disk-policy-and-menubar.md`, `0005_disk_policy.sql` (`disk_entry`, `keep_rule`).
- **Engines & Unified Queue (`cinma-http`, `cinma-torrent::download`, `cinma-player::mux`, `cinma-core::downloads`)**:
  - Segmented HTTP/1.1 downloader (`http1_only`, dynamic gap splitting, per-host cap, `ETag`/`Last-Modified` validators, `PriorityListener` read-cursor steering, verified against real `kill -9`).
  - Full torrent download engine with `fastresume`, upload/download caps, seeding policy (`off` / `ratio` / `minutes`), and Darwin `malloc_zone_pressure_relief` on stop.
  - `yt-dlp -J` download engine (video+audio, capped height, M4A audio-only) + in-process FFmpeg muxing (`remux_files` via `dlopen` `libavformat.63`/`libavcodec.63`).
  - Live mid-download **Play Now** via `Blob::source()` -> `Playback::play_download`.
- **Disk Policy & Menu Bar (`Core::disk`, `DownloadsViewController`, `MenuBarController`)**:
  - `NSTableView` + `NSTableViewDiffableDataSource` queue UI, per-item & global "Keep Offline" rules, LRU quota eviction, `NSStatusItem` menu bar extra with `ProcessInfo` App Nap activity only while downloads are active, and `applicationShouldTerminateAfterLastWindowClosed` toggleable behavior.

### Done in Phase 6 (Optional Integrations — Off by Default, Zero Idle Cost)
- **ADR & Migration**: `docs/adr/0014-casting-and-tracking-outbox.md`, `0006_tracking_outbox.sql` (`sync_outbox`, `sync_adapter_state`).
- **Casting (`cinma-casting`, `IntegrationsView.swift`)**: On-demand SSDP (DLNA AVTransport) & `dns-sd` (Chromecast CastV2 TLS/protobuf) discovery + ephemeral LAN HTTP `Range` server active *only* during cast sessions; Player HUD Cast device picker + remote play/pause/seek/stop. AirPlay video limitation recorded in `docs/RISKS.md`.
- **Tracking & Imports (`cinma-tracking`, `Core::integrations`)**: SQLite outbox + exponential backoff worker, Keychain per-profile tokens, Trakt device-code OAuth/scrobble/recommendations, Simkl, AniList GraphQL, MAL, Letterboxd RSS & CSV export, bundled Anime ID starter map, and one-time Stremio addon + library import.

### Done in Phase 7 (Polish, Security, Backup & Packaging)
- **ADR**: `docs/adr/0015-packaging-and-custom-libmpv.md`.
- **Security & Backup (`Core::security_and_backup`)**: Profile PINs hashed/verified with `PBKDF2-HMAC-SHA256` (`ring`, 100k iterations, random 16B salt) + PIN prompt/management in `ProfilePickerView`; SQLite online snapshot export (`VACUUM INTO`) & integrity-validated live restore UI in Settings.
- **UX, Accessibility & Packaging (`DesignSystem.swift`, `SheetsAndSettings.swift`, `tools/package-release.sh`, `Cinma.entitlements`)**: Actionable friendly errors (`FriendlyError`), polished 3-step Onboarding, VoiceOver accessibility labels, `Contents/Frameworks/libmpv.2.dylib`-first loader path, and signed `.app` packaging script.

### Verification & Measurements (Phases 5–7)
- **`cargo fmt --check` & `cargo clippy --all-targets -- -D warnings`:** passed (0 warnings).
- **`cargo test`:** **100 Rust tests pass**, 0 failed, 0 ignored across all workspace crates (`blobstore`, `db`, `player`, `queue`, `http`, `torrent`, `casting`, `tracking`, `stremio`, `metadata`, `ranker`, `subtitles`, `core`).
- **Release `xcodebuild` (`Cinma.app`):** succeeded with **0 errors and 0 warnings**.
- **Launch & Idle (`tools/measure.sh`, Release, 5 runs):**
  - Median first frame: **181.8 ms** (< 250 ms target ✅)
  - Median idle `phys_footprint`: **30.0 MB** (≤ 40 MB target ✅), RSS **100.6 MB**
  - Median idle CPU: **0.00 %** (~0% target ✅)
- **Closed-window Menu Bar idle:** `28 MB` `phys_footprint`, `0.0 %` CPU.
- **Packaged Bundle (`tools/package-release.sh` -> `dist/Cinma.app`):** `codesign --verify --deep --strict` clean; 1080p MKV playback smoke test (`CINMA_AUTOPLAY=file:...`) TTFF **401.5 ms**, seek **88.6 ms**, `gamma=bt.1886 hdr=false`, exit 0.

## Phase 4 — Player complete (awaiting acceptance)

### Done in Phase 4
- **ADR 0011** ([0011-phase4-player-thumbnails-and-subtitles.md](docs/adr/0011-phase4-player-thumbnails-and-subtitles.md)): seek-bar thumbnails via runtime `dlopen` of Homebrew FFmpeg (`libavformat.63`, `libavcodec.63`, `libswscale.10`, `libavutil.61` with major-version verification + AVFoundation fallback), OpenSubtitles hash + Turkish encoding strategy, dedicated CA GL thread, SDR 8-bit surface path, and PiP feasibility spike.
- **Dedicated GL render thread & SDR 8-bit optimization (`cinma-player`, `VideoLayer.swift`, `PlayerHelpers.swift`)**:
  - `VideoLayer` switched to `isAsynchronous = true` with `MPV_RENDER_PARAM_ADVANCED_CONTROL = 1`, `vd-lavc-dr=yes`, `mpv_render_context_update()` in `canDraw`, `MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME = 0`, and `CGLLockContext` synchronization — eliminating the `@unchecked Sendable` compiler warning too.
  - Known SDR playback uses an 8-bit `ITUR_709` surface (`rgba8` / `bt.1886`), keeping 16-bit float PQ/BT.2020 EDR for HDR/DV streams; demuxer defaults trimmed to `16MiB` / `4MiB`.
- **Seek-bar keyframe thumbnails (`cinma-player::thumbnails`, `PlayerHelpers.swift`)**:
  - Added non-blocking `ByteSource::try_read_at` & `available_ranges` across `cinma-blobstore` (`BlobSource`, `MemorySource`) and `cinma-torrent` (`TorrentSource` reads tracked ranges from disk without touching `FileStream::seek`), ensuring hovering never triggers network/peer requests.
  - ~160 px RGBA keyframe decoder with bounded LRU cache + Swift `NSCache` hover preview + chapter markers in `ScrubBarView`.
- **Full subtitles pipeline (`cinma-stremio`, `cinma-subtitles`, `cinma-core`, `cinma-ffi`, `PlayerMenus.swift`)**:
  - Stremio `subtitles` resource support (`videoHash`, `videoSize`, `filename`); 64 KiB head+tail OpenSubtitles hash computed once available in `ByteSource` or local files; optional direct OpenSubtitles REST API v1 client (credentials in Keychain `app.cinma.mac`).
  - Gzip unpack (`flate2`) + encoding detection (`encoding_rs`) converting Turkish `windows-1254` / `ISO-8859-9` & UTF-16 subtitles into cached UTF-8 files while leaving ASS/SSA script styling untouched unless overridden.
  - Language-priority auto-select, dual subtitles (`secondary-sid`), persisted per-video `sub_delay`/`audio_delay`, external subtitle loading, and SRT style controls.
- **Complete playback controls, Next Episode, Recovery, Settings & macOS Integration**:
  - Audio/subtitle/chapter menus, Skip Intro button, Next Episode prefetch at ~88–92% (matching `bingeGroup` + head/tail warm-up on cancellation token) with auto-play countdown card, live Stream Switcher + stall/error auto-recovery fallback, A/B loop, playback speed, volume/mute, sleep timer, screenshot export, live stats overlay, adaptive CAS GLSL shader preset, HDR `target-peak` control, exact relative seeks toggle, `MPNowPlayingInfoCenter` + `MPRemoteCommandCenter` media keys, display-sleep power assertion, all 17 player shortcuts via `CommandRegistry`, visual `KeyCaptureButton` recorder in Settings, and `en` + `tr` translations (`Localizable.xcstrings`, 205 keys).
- **PiP feasibility spike (report only, per ROADMAP)**:
  - `AVPictureInPictureController` on macOS requires `AVPlayerLayer` or `AVSampleBufferDisplayLayer`; attaching it over our `CAOpenGLLayer` render path (`ADR 0004`) would require per-frame `CVPixelBuffer` → `CMSampleBuffer` copying, adding copy overhead and losing direct HDR/EDR/libass parity. An AppKit floating borderless `NSPanel` mini-player would reuse `VideoLayer` with zero copy. Awaiting owner preference before implementing.

### Verification & Measurements (Phase 4)
- **`cargo clippy --all-targets -- -D warnings`:** passed (0 warnings).
- **`cargo test`:** **88 Rust tests pass**, 0 failed, 0 ignored (`blobstore` 21, `core` 14, `db` 16, `metadata` 1, `player` 5 incl. real MKV FFmpeg thumbnail test, `queue` 7, `ranker` 6, `stremio` 10, `subtitles` 4, `torrent` 4).
- **Release `xcodebuild`:** passed with **0 errors and 0 warnings**.
- **Launch & Idle (`tools/measure.sh`, Release, 5 runs):**
  - Median first frame: **170.5 ms** (< 250 ms target ✅)
  - Median idle `phys_footprint`: **28.0 MB** (≤ 40 MB target ✅), RSS **97.4 MB**
  - Median idle CPU: **0.00 %** (~0% target ✅)
- **Playback measurements (`Cinma` Release):**
  - 4K HEVC HDR10 (`hdr10_hevc_4k.mp4`): **7.3 % CPU** (< 10% target ✅, down from ~19%), TTFF **143.1 ms**, `phys_footprint` **543 MB** (peak 572 MB), `gamma=pq hdr=true`, clean exit 0.
  - 1080p SDR H.264 (`sdr_h264_1080p.mkv`): **7.1 % CPU** (down from ~21%), TTFF **383.9 ms** / seek **79.6 ms & 189.7 ms**, `phys_footprint` **378 MB**, `gamma=bt.1886 hdr=false`, clean exit 0.

## Phase 3 — Browsing & stream selection ✅ (accepted 2026-11-17)

### Done in Phase 3
- **Deep-link confirmation** (`AppDelegate.handleLink`): `stremio://` links inspect the manifest via `Core::inspect_addon` (without persisting) and ask confirmation in an `NSAlert` showing addon name, description, and URL (`en` + `tr`).
- **Torrent streaming & demuxer tuning** (`cinma-torrent`, `cinma-player`): tuned `librqbit` `SessionOptions` (`peer_limit: Some(80)`, `connect_timeout: 4s`, `read_write_timeout: 8s`, `max_request_window: Some(16)`), fallback trackers when not `local_only`, `AddTorrent::from_bytes` + `seen_peers` reuse on `list_only`, background `spawn_head_tail_warmup`, and trimmed mpv `demuxer-max-bytes` (`24MiB`) / `demuxer-max-back-bytes` (`8MiB`).
- **Addon protocol (`cinma-stremio`, `cinma-core`, `cinma-ffi`)**: added `catalog` (with `search`, `genre`, `skip` extras), `meta` (movies, series, episodes, trailers, links), catalog listing/browsing/search fan-out, and per-addon incremental batch observers (`StreamBatchObserver`, `CatalogBatchObserver`) tested against offline Cinemeta/Torrentio-shaped JSON fixtures.
- **ADR 0010 & Schema 0003/0004 (`cinma-db`)**: accepted `docs/adr/0010-metadata-and-library-schema.md`; added migrations `0003_metadata.sql` (`id_map`, `meta_cache`) and `0004_library_and_progress.sql` (`library_item`, `watch_progress`), profile CRUD (`create_profile_with_avatar`, `rename_profile`, `delete_profile`), and `reorder_addons`.
- **Metadata, Keychain, Library/Progress & `yt-dlp` (`cinma-metadata`, `cinma-core`, `cinma-ffi`)**: TMDB v3 primary client with Keychain API key storage (`app.cinma.mac` / `CINMA_TMDB_API_KEY`) and SQLite `meta_cache` + Stremio fallback; Continue Watching & auto-watched (`>= 0.90` of `duration_s`); short-lived `yt-dlp -J` trailer extraction and user-triggered install/update.
- **Stream ranker (`cinma-ranker`)**: parse → filter (`Strict`/`Standard`/`Permissive`) → score (resolution, source, HDR/DV fallback, codec with M3+ hardware AV1 detection via `sysctl`, audio, seeders/debrid, preferred languages, reputable release groups) → tier (`Best`/`Recommended`/`Good`/`Fallback`) → explainable `ScoreReason` breakdown, wired into `Core` and `FFI`.
- **Native macOS Browsing & Stream Selection UI (`app/Cinma/`)**:
  - `DesignSystem.swift` & `ImagePipeline.swift`: Cinema Dark (`#0B0B0D`, `#E8A33D`, `#5CC98A`) + native `NSGlassEffectView` (macOS 26+ with `NSVisualEffectView` & Reduce Transparency/Motion fallbacks), ImageIO downsampling, 64 MB `NSCache` (cleared on window close), 150 MB LRU disk cache.
  - `MainShellViewController.swift`, `HomeBrowserViewController.swift`, `DiscoverViewController.swift`, `TitleDetailViewController.swift`, `LibraryCalendarViewControllers.swift`, `PosterCards.swift`, `SheetsAndSettings.swift`, `PlayerViewController.swift`: SQLite-first Home launch (Continue Watching + `cached_trending` before network), rotating Hero, Discover (catalogs/genres/skip pagination/incremental search), Title Detail (seasons/episodes, `yt-dlp` trailer, ranked stream picker with quality/tier/language explanations), Library, Calendar, Addon Manager, Profile Switcher, Onboarding, Settings, and floating Liquid Glass player HUD with resume/progress tracking.
  - `Localizable.xcstrings`: 129 `en` + `tr` strings; legacy `SliceViewController.swift` removed.
- **Verification & measurements**:
  - `cargo clippy --all-targets -- -D warnings`: pass (0 warnings).
  - `cargo test`: **78 Rust tests pass** (0 failed, 0 ignored) across workspace crates (`blobstore` 19, `core` 13, `db` 16, `metadata` 1, `player` 3, `queue` 7, `ranker` 6, `stremio` 9, `torrent` 4).
  - Release `xcodebuild`: clean build (only pre-existing `VideoLayer.swift:12` `@unchecked Sendable` warning).
  - `tools/measure.sh` (Release, 5 runs): median first frame **170.2 ms** (< 250 ms), idle `phys_footprint` **29.0 MB** (≤ 40 MB), idle CPU **0.00 %**.
  - Smoke playback (`sdr_h264_1080p.mkv` in fresh `CINMA_DATA_DIR`): clean exit, `CINMA_FIRST_FRAME_MS=155.4`, `CINMA_TTFF_MS=253.0`.

## Phase 2 — Core solid ✅ (accepted 2026-11-17)

### Done in Phase 2
- **Schema 0002** (ADR 0008): `addon` + `profile_addon` (installed globally, enabled per profile), `profile_setting`, `download`; profile gets `avatar` and `pin_hash`.
  - The Phase 1 addon list migrates automatically; tested on a v1 database with data.
  - Repositories are profile-scoped; an isolation test covers it.
  - `busy_timeout` 5 s.
- **Settings and feature toggles**: `addons` and `torrent`, stored in `app_setting`.
  - A disabled feature is never constructed: tested that no runtime, HTTP client or torrent session gets created.
  - Turning torrent off during playback stops the session, so the playing torrent stops receiving data. This is unit-tested in Rust; there's no UI toggle yet, so it hasn't been run in the app.
- **BlobStore** (ADR 0009):
  - sparse files plus a durable ranges sidecar;
  - write and mark-available are separate steps;
  - `checkpoint` (fsync, then atomic sidecar);
  - blocking reads wake on data or cancel without polling (cancel returns in < 50 ms);
  - a priority hint on every read;
  - head/tail-first `fetch_order`.
  - `MemorySource` no longer polls.
- **Download queue skeleton** (`cinma-queue`):
  - priority scheduler with `max_active`, a `DownloadEngine` trait, pause/resume/remove;
  - `active`→`queued` recovery on start-up and resume from the blob's ranges;
  - progress is checkpoint-then-DB;
  - fixed a pause→resume race (job generations).
  - Engines come in Phase 5; the queue isn't wired to the UI yet.
- **kill -9 tests** (real child processes): blob bytes always match the sidecar; a download row never runs ahead of its blob; SQLite `integrity_check` passes.
- **Command registry**: every menu item, the player keys and the `stremio://` deep link go through command ids.
  - Bindings: defaults plus user overrides (`keybinding.*`); conflicts within a context are rejected.
  - Launch builds the menu from static defaults (no DB) and rebuilds with the user's bindings once the core is open.
  - Verified in the app:
    - the menu, rebuilt from the registry (listed via accessibility);
    - player keys ← / → / Esc resolving through the registry;
    - a `stremio://` link installing Cinemeta, driven through the app's URL handler by a dev hook. macOS's own open-URL path is not exercised.
- **Core event stream**: `AddonsChanged` and `KeyBindingsChanged`, the only two with consumers. A Swift listener refreshes the addon label and the menu.
- `stremio://` URL scheme registered in Info.plist, so macOS may offer Cinma for Stremio links.

### Measured
| Metric | Value | Target |
|---|---|---|
| Launch, first frame (median of 5) | 154 ms | < 250 ms ✅ |
| Idle footprint | 25 MB | ≤ 40 MB ✅ |
| Idle CPU, before any playback | ~0 % (≤ 0.07 %, the `ps` resolution) | ~0 % ✅ |
| Idle CPU, after a torrent playback is closed | ~0.13 % (0.04 s CPU in 30 s, 15 s after close); cause not identified (tokio/hyper timers suspected) | ~0 % ⚠️ |
| Tests | 60 Rust tests (incl. 2 kill -9 suites), clippy clean, Swift build warning-free | — |

### Re-verified after the refactor
- Torrent slice end to end on loopback: TTFF 205 ms, seek 1.3 s; streamed data cleared on close.
- Run on a copy of a data dir written by the Phase 1 build: v1 → v2 migration kept the addon and enabled it for profile 1.
- Debug build: launches and plays a torrent; Info.plist merge OK.

### Decisions (owner delegated, 2026-11-17)
1. ADR 0008 schema: accepted. Addon identity is the manifest id, as in Stremio.
2. ADR 0009: torrents stay on librqbit storage: accepted.
3. `stremio://` links must ask for confirmation before installing. This is the first task in ROADMAP Phase 3.

---

## Phase 1 — Vertical slice ✅ (accepted 2026-11-17)

### Done in Phase 1
- **Addon client** (`cinma-stremio`): manifest/stream parsing (Torrentio-shaped fixtures), `stremio://` and URL normalization, `supports()` by type and idPrefix. Installed addons persist as JSON in `app_setting` (`addons.v1`); real tables come with the Phase 2 schema.
- **Streams**: every installed addon that serves the type/id is queried in parallel. Results keep addon order. Ranking is Phase 3.
- **Torrent** (`cinma-torrent`, librqbit behind `TorrentEngine`):
  - lazy session; DHT state under the data dir; with no `fileIdx`, the largest video file is picked;
  - `release_all` on stop, so idle CPU goes back to ~0 % and no sockets stay open;
  - offline loopback test (ADR 0006).
- **ByteSource** (`cinma-blobstore`): readers with cancellable blocking reads. Sparse blobs and bitmaps are Phase 2.
- **Player bridge** (`cinma-player`):
  - libmpv is dlopened on first playback, never linked (ADR 0005);
  - the `cinma://` protocol uses `mpv_stream_cb_add_ro` with read, seek, size, close and cancel; no localhost server;
  - the event thread blocks on `mpv_wait_event`, with no polling;
  - render API on `CAOpenGLLayer`, fixed PQ/EDR surface (ADR 0004 — **owner decision pending**: this path can't do Dolby Vision).
- **App**: Phase-1 screen (install addon → find streams → double-click to play, plus Open File…) and a player with a control bar (space, ←/→ ±10 s, Esc). Standard Edit menu, so text fields get ⌘C/⌘V. English and Turkish strings.
  - Verified through the real UI: install, find streams, double-click, playback, Esc.
- **Robustness** (loopback):
  - a stalled source, a seek into missing data and then close: `Shutdown` arrives in 0.12 s, with no hang;
  - a seek blocked while the seeder is down resumes when the seeder comes back;
  - streamed data is deleted when playback stops (no disk policy yet, Phase 5).
- **Tools**: `tools/measure-playback.sh` (TTFF and seek through the real app); env-driven dev automation (`DevAutomation.swift`); a `local_seed` example.

### Measured
| Metric | Value | Target |
|---|---|---|
| Launch, first frame (median of 7) | 158 ms | < 250 ms ✅ (floor ~152 ms) |
| Idle footprint, window open | 25 MB | ≤ 40 MB ✅ |
| Idle CPU, before and after playback | ~0 % | ~0 % ✅ |
| TTFF, loopback seeder at 6 MB/s, from play request (median of 5) | 192 ms | — |
| Seek into undownloaded region, loopback 6 MB/s (median) | 953 ms | — |
| **TTFF / seek on a real swarm** | **not measured** | < 5 s / < 2 s |
| 4K HDR10 playback CPU | ~19 % | < 10 % — owner accepted for now |
| Footprint after a torrent playback ends | ~113 MB | ≤ 40 MB — owner: look at it later |

Real-swarm numbers are missing because the agent's sandbox proxies outbound TCP, so it can't reach BitTorrent peers. To measure on a normal network:
`tools/measure-playback.sh <addon manifest url> movie <imdb id> 5`

### Risks / open items
1. **Real-swarm TTFF and seek** are unverified. librqbit streams with a 32 MB window ahead of the cursor and prioritizes the first and last pieces. Whether that meets < 5 s / < 2 s on real swarms is unknown.
2. **Memory after torrent playback** (owner: later): footprint stays at ~113 MB, while the live heap is only 13.5 MB. The rest is malloc fragmentation in the small zone; `malloc_zone_pressure_relief` frees ~0. Solution paths, cheapest first:
   - find librqbit's large transient buffers (per-peer read buffers, chunk caches) and cap them via options or our storage factory;
   - run the torrent engine's allocations in a separate malloc zone, or a Rust global allocator that returns memory eagerly (mimalloc with `purge_delay=0`, or jemalloc with `dirty_decay_ms`), measured before/after (dependency ADR);
   - keep the torrent session alive while playing, and when it stops, accept the cost until memory pressure (macOS reclaims under pressure).
3. **Playback CPU** is ~20 %. **Playback footprint** is 470–630 MB. Both are Phase 4 (ADR 0004).
4. **Render path is an open decision** (ADR 0004): the current OpenGL render API uses the legacy renderer, which means no Dolby Vision P5, no HDR peak detection, and deprecated OpenGL. **Homebrew libmpv** is a stand-in until a custom build (ADR 0005).
6. **Uploading:** while streaming, librqbit also uploads to peers (its default). Seeding policy and rate limits come in Phase 5.
5. Rendering happens on the main thread. A long `mpv_render_context_render` would stall the UI. Move it to a dedicated thread in Phase 4.

Real-swarm TTFF and seek are still unmeasured. Owner decisions: stay on the OpenGL render path; ~19 % CPU accepted; post-torrent memory deferred.

---

## Phase 0 — Skeleton ✅ (accepted 2026-11-17)
Rust workspace, UniFFI wiring, SQLite migrations, AppKit shell, String Catalog, `tools/measure.sh`. Launch 150–160 ms, matching the bare-AppKit floor of ~152 ms. Idle footprint 20 MB. ADRs 0001–0003.
