# PROGRESS

## Status: Phases 0–9 + Harbor Settings Parity (Waves 1–5 Lightweight) complete (2026-11-18). Waiting for owner device tests and Phase 10 hardening.

**Next:** see [README.md](README.md) and `docs/ROADMAP.md`:
- §2: the owner's real-network / device test checklist;
- §3 & §6: recorded owner decisions (2026-11-17);
- §9: Phase 10 (hardening, bundled `libmpv`+FFmpeg, `mimalloc`/`jemalloc` fragmentation trial, crash sweep, competitor benchmark).

Unmeasured outside local mocks / sandbox: real-swarm torrent TTFF/seek and live external service round trips (Debrid, OAuth, AniSkip/IntroDB, Fanart, Kitsu, IPTV, Discord IPC, Chromecast/DLNA). See `docs/RISKS.md`.

---

## Review of the polish/refactor work (2026-11-19)
Reviewed P1–P5 (`fad08d4`…`056abe2`) and Antigravity's audit (`~/.gemini/antigravity/brain/49e82e83…/polish_refactor_audit.md`). Fixes:
- **Torrent rate limits change live** (`session.ratelimits`) instead of stopping the session, which cut off playback and failed active torrent downloads whenever a limit changed in Settings. Only a VPN interface change still restarts the session. Loopback test added.
- **Download removal race:** a job that created its blob just as the download was removed left an orphaned data file. `Job::blob` and `Queue::remove` now close that gap. "Play Now" falls back to the finished file if the blob was moved at that instant.
- **External players** open through URL schemes or Launch Services (an `.app`), never as Cinma's child process (§1.1).
- **Home rails** get their height from the card height. Collection-view layout warnings dropped from 20 to 4; the rest come from the first layout pass.
- **Local socket** requests are capped at 64 KiB.
- **Flaky tests** (fixed sleeps) now wait for state. The full suite passed 147/147 three times.
- **Docs:**
  - ozellikler.md: #68 is marked removed; the #70 and #16 descriptions are corrected; paths updated after the refactor.
  - README: benchmarks re-measured; unverifiable 7.x % CPU claims removed.
  - 20 stale strings dropped.
- **Measured** (Release, 2026-11-19):
  - launch 173 ms;
  - idle 34 MB;
  - Home with artwork 135 MB at 0 % CPU;
  - 4K HDR10 ~15 % CPU.
- **4K HDR10 footprint depends on window size:** 590 MB in a 1280×800 window, 683–689 MB in a 1920×975 window on a 4K external display. Both are within the 700 MB limit; the higher figure is not a code regression.


## Post-Phase 9 — Player UX, Memory Trim & Harbor Settings Parity Waves 1–5 ✅ (2026-11-17 – 2026-11-18)

### Done after Phase 9 (`bf1dc3a`..`1ef7e4d`)
- **Playback RAM & UI latency fixes (`bf1dc3a`):** moved HUD controls to bottom glass bar, optimized browse/artwork caching (`ImagePipeline.setPlaybackMode`), lowered live 4K streaming footprint to **~460 MB** and 1080p torrent playback to **~370 MB** (~260 MB browse/idle in real-world sessions), and reduced Cast stop latency.
- **Toast, Stats & Subtitle Controls (`4bc776d`) + Cross-Episode State (`b1354c5`):** auto-dismissing player toast notifications, fixed wrapping stats overlay, fixed Ep 1 → 2 → 3 auto-advance countdown re-arming bug, and preserved audio/subtitle/video selections cleanly when auto-advancing episodes.
- **Streaming Details Globe & Fullscreen `Esc` (`87304d9`):** added live `globe` Streaming Details popover (`0.5s` download/upload speed, active/total peers, buffered bytes, infoHash, `hwdec`, cache seconds, dropped frames) and updated `Esc` to dismiss popovers → exit macOS fullscreen → close window cleanly.
- **Harbor Settings Parity — Wave 1 (`d3b65d2`):** Auto-skip Intro/Recap/Outro, lift subtitles above HUD controls, pause on minimize/unfocus, customizable seek step (`3–60s`), `dynaudnorm` audio normalization & stereo downmix, subtitle opacity/box opacity/alignment, secondary sub placement, aspect/crop modes (`fit`, `fill`, `16:9`, `21:9`, `4:3`), default off / prefer embedded / forced-on-native subtitles, max volume boost (`100/150/200%`), default/custom playback speed, and configurable stall watchdog timeout.
- **Harbor Settings Parity — Wave 2 (`3653daa`):** Episode Spoiler Protection (blurred unwatched stills + hidden descriptions), per-series source lock/remembering, release-group keyword blocking (`trackBlockWords`), stream sorting (`score`, `seeds`, `size_desc`, `size_asc`), filename toggle, SDH subtitle track/cue stripping, HUD clock + estimated finish time, Up Next lead time + "Still Watching?" threshold, hide watched/unreleased catalog items, ascending/descending & compact episode list, confirm-on-leave, resume prompt modes (`always_resume`, `ask`, `always_start_over`), volume HUD toast, and player screen lock.
- **Harbor Settings Parity — Wave 3 (`e971a9e`):** HDR-to-SDR tone mapping (`auto`, `bt.2390`, `reinhard`, `hable`, `clip`), macOS EDR toggle, `yuv420p` toggle, mpv scaler preset (`fast`, `balanced`, `high`), `hwdec` mode (`videotoolbox`, `auto-safe`, `no`), `mpv` buffer size (`4–128 MiB`), guarded extra mpv options, Debrid-only mode (hide P2P torrents), configurable addon HTTP timeout (`3–60s`), torrent download limit (`download_bps`), Home CW-at-top & addon rail cap, CW hide nearly-finished (`≥ 85%`), poster scale/radius/hide-title & card badges (IMDb, type, watched, in-library), seek bar chapters, title order swap, calendar Monday start, Discord privacy toggles, custom sleep timer, P2P speed chip next to Globe, player always-on-top, and in-app EN/TR language selector.
- **Harbor Settings Parity — Waves 4 & 5 Lightweight Only (`1ef7e4d`):** deliberately skipped heavy/GPU-costly features (Anime4K shader packs, auto-playing detail trailers, full-episode pre-downloading, background webhook daemon, gamepad/10-ft TV mode) and added zero-overhead settings: ASS/SSA & subtitle scale (`50–150%`, `sub-ass-scale-with-window`), OpenSubtitles/addon subtitle provider toggles, TMDB original title & image language preferences + expanded metadata languages, on-demand TMDB Streaming Platform filters in Discover (Netflix, Disney+, Apple TV+, Prime Video, Max, Crunchyroll, MUBI), dynamic accent color presets (`amber`, `crimson`, `cyan`, `emerald`, `violet`), GPU-saving Solid Glass (`reduceGlassEffect`) toggle, detail backdrop dim control, top-nav tab visibility toggles, external `mpv`/`IINA` handoff for direct HTTP/Debrid streams, and deep-link confirmation / scrobble toggles.

### Verification (After Latest Wave 4 & 5 Build — `1ef7e4d`)
- **`cargo fmt --check` & `cargo clippy --all-targets -- -D warnings`:** passed (0 warnings).
- **`cargo test --workspace --quiet`:** **139 Rust tests pass, 0 failed, 0 ignored** across 56 test suites.
- **Release `xcodebuild` (`Cinma.app`):** **`** BUILD SUCCEEDED **`**, **0 compiler warnings**.
- **Localization (`app/Cinma/Localizable.xcstrings`):** **727 translatable keys (`en` + `tr`)**, `MISSING_TR_COUNT=0`.

---

## Phase 9 — Integrations & add-ons ✅ (2026-11-17)

### Done in Phase 9 (`docs/adr/0017-phase9-integrations-and-providers.md`, migration `0008_phase9_integrations.sql`)
- **Zero new external Cargo dependencies:** reused existing workspace crates (`reqwest`, `serde`, `ring`, `url`, `tokio`, `rusqlite`, `librqbit`).
- **Off-by-default runtime boundaries (`Feature::{SkipSegments, Debrid, Iptv, Discord}`):** disabled modules construct zero HTTP clients, sockets, or Live TV UI views and tear down immediately when toggled off.
- **Skip intro/outro (`cinma-metadata::skip_segments`, `skip_segment_cache`):** AniSkip v2 (`mal_id`) + IntroDB (`imdb_id`, season, episode) with 7-day SQLite TTL, merged directly into `PlayerViewController`'s Skip button alongside chapter markers.
- **Debrid resolvers (`crates/features/debrid` + `cinma-ranker`):** Real-Debrid, AllDebrid, Premiumize, and TorBox Keychain API keys, batch instant-cache lookup, ranker `[Instant Debrid]` +`160` boost, and HTTPS stream resolution on play with P2P fallback.
- **OAuth PKCE, Two-Way Trakt & Anime ID Refresh (`cinma-tracking::oauth`, `Info.plist`):** registered `cinma://` URL scheme for system-browser OAuth PKCE/code exchange (`Simkl`, `AniList`, `MAL`, zero WebViews), two-way Trakt pull (`/sync/watchlist`, `/sync/ratings`, `/sync/history`), and Fribb-compatible `refresh_anime_id_map`.
- **OpenSubtitles account & hash search (`cinma-subtitles`, `SheetsAndSettings.swift`, `PlayerViewController.swift`):** `/login`, `/infos/user` quota display, `/logout`, plus hash-based subtitle lookup enabled on both stream and local files.
- **Metadata enrichment (`cinma-metadata`):** Fanart.tv v3 (`clearlogo` + backdrops via Keychain key), TMDB watch providers (`Watch on: ...` on `TitleDetailViewController`), and Kitsu JSON:API `kitsu:<id>` fallback.
- **Store-mode Archive playback (`crates/blobstore/src/archive.rs`):** `ArchiveByteSource: ByteSource` streaming uncompressed entries (`method == 0` ZIP + `0x30` RAR4/RAR5) over any outer torrent/file `ByteSource` with seek priority forwarding.
- **Live TV / IPTV (`crates/features/iptv`, `LiveTvViewController.swift`):** M3U/M3U8, Xtream Codes, and XMLTV EPG parsers cached in SQLite; conditional `Live TV` tab shown only when `Feature::Iptv` is enabled and played directly through `libmpv`.
- **Discord Rich Presence (`crates/features/discord`):** in-process `$TMPDIR/discord-ipc-{0..9}` Unix domain socket client updating on play/pause/seek/stop.
- **VPN interface binding & kill switch (`cinma-torrent`, `disk.rs`, `SheetsAndSettings.swift`):** BSD interface picker (`Darwin.getifaddrs`) + `librqbit::SessionOptions::bind_device_name` + fail-closed kill switch.
- **macOS integrations (`CalendarNotifications.swift`, `SpotlightAndIntents.swift`):** local-only `UserNotifications` for upcoming library episodes (off by default, zero daemon) + `AppIntents` (`OpenCinmaSearchIntent`, `PlayContinueWatchingIntent`) + opt-in lazy `CoreSpotlight` library indexing.

### Verification & Measurements (Phase 9, Release, after final edits)
- **`cargo fmt --check` & `cargo clippy --all-targets -- -D warnings`:** passed (0 warnings).
- **`cargo test --quiet`:** **132 Rust tests pass**, 0 failed, 0 ignored (`blobstore` 28 incl. store ZIP/RAR4 archive streaming, `core` 20 incl. `phase9` zero-cost/VPN/PKCE tests, `debrid` 2, `discord` 1, `iptv` 2, `metadata` 3, plus existing suites).
- **Release `xcodebuild` (`Cinma.app`):** passed with **0 compiler warnings** and **0 errors**; `501` `en` + `tr` localized strings (`0` missing non-empty translations).
- **Launch & Idle (`tools/measure.sh`, Release, 5 runs):**
  - Median first frame: **184.2 ms** (< 250 ms target ✅)
  - Median core ready: **193.5 ms**
  - Median idle `phys_footprint`: **31.0 MB** (≤ 70 MB empty window target ✅), RSS **100.4 MB**
  - Median idle CPU: **0.00 %** (~0 % target ✅)
- **Local playback smoke tests (Release):**
  - 1080p SDR MKV (`sdr_h264_1080p.mkv`): TTFF **422.0 ms**, seek **79.8 ms**, `gamma=bt.1886 hdr=false`, exit `0`.
  - 4K HDR10 MP4 (`hdr10_hevc_4k.mp4`): TTFF **350.5 ms**, seek **139.6 ms**, `gamma=pq hdr=true`, exit `0`.

---

## Phase 8 — UI/UX polish & zero-copy mini-player PiP ✅ (2026-11-17)

### Done in Phase 8
- **Actionable errors (`FriendlyError`)**: unwrapped raw `Cinma.CinmaError` strings across stream picker, detail, browsing, downloads, addons, and settings into plain human messages with concrete `actionHint` guidance and inline recovery CTAs (`Open Addons…`, `Open Settings…`, `Clear Search`, `Explore Home`, `Retry`).
- **Downloads UX & in-memory live progress (`cinma-queue`, `cinma-torrent`, `cinma-core`, `DownloadsViewController.swift`)**:
  - `LiveProgressMap` tracks active transfer bytes and `Blob::available().covered()` in RAM (`1 s` live ticks; SQLite checkpoints remain coarse every `5` ticks / ~`5 s`), cleared on finish/stop.
  - Row height raised to `72 pt`, action column widened to `290 pt` with vertically centered controls so pill buttons (`Play Now`, `Pause`/`Resume`, `Show in Finder`, `Remove`) no longer clip.
- **Home hero & image memory (`HomeBrowserViewController.swift`, `DesignSystem.swift`, `ImagePipeline.swift`)**:
  - Replaced `heroFallbackGradient` CALayer with auto-laying-out `GradientLayerView` + multi-stop full-bleed `leftScrim` / `bottomScrim` (eliminates half-width edge artifacts).
  - Pauses the 10 s hero rotation timer via `NSWindow.didChangeOcclusionStateNotification` and `NSApplication.didResignActiveNotification`.
  - Caps decoded backdrops (`960×420` / `960×380`, `min(1920, ...)`) and lowers `ImagePipeline` memory limit from `48 MB` (active) to `16 MB` (inactive).
- **Onboarding & Tabbed Settings (`SheetsAndSettings.swift`, `crates/core/src/metadata.rs`)**:
  - Core metadata fallback changed from `"tr-TR"` to `"en-US"` and `OnboardingView` matches `Bundle.main.preferredLocalizations` (`en-US` vs `tr-TR`), plus addon URL scheme validation (`http://`, `https://`, `stremio://`) and TMDB key verification via `verifyTmdbKey`.
  - Split flat `SettingsView` into 6 tabs (`General`, `Playback`, `Subtitles`, `Downloads`, `Shortcuts`, `Backup`) showing download folders and `yt-dlp` status clearly.
- **Empty, loading, partial states, keyboard navigation & VoiceOver**:
  - Added spinners, skeleton/empty recovery views, and incremental `"m of n addons answered · k streams"` progress in `TitleDetailViewController`.
  - Added `KeyboardCollectionView` (arrow keys + `Return`/`Space`), `PillButton.drawFocusRingMask()`, accent (`#E8A33D`) 2.5 px focus rings on `PosterCardItem` / `ContinueWatchingCardItem`, `Esc` (`cancelOperation`) handling, and VoiceOver labels/descriptions on cards, hero dots, and settings controls.
- **Player HUD polish & zero-copy PiP `NSPanel` (`DesignSystem.swift`, `PlayerViewController.swift`, `MiniPlayerPanel.swift`)**:
  - Added `GlassKind.hud` with a dark scrim backing behind `NSGlassEffectView` so HUD pills/icons stay legible over bright HDR highlights, smoothed auto-hide/scrubber thumb clamping (`isHoveringScrub`), and shipped the floating always-on-top `MiniPlayerPanel` (`NSPanel`, `P` shortcut + HUD toggle) reusing `VideoLayer` with zero frame copies.
- **App icon, About window & `en` / `tr` localization (`AppIcon.icns`, `Info.plist`, `MainMenu.swift`, `Localizable.xcstrings`)**:
  - Added multi-resolution macOS squircle `AppIcon.icns`, About / Licenses window (`app.about` menu item listing `libmpv`, FFmpeg LGPL, `librqbit`, etc.), and synced/translated all `408` `en` + `tr` strings (`0` missing).

### Verification & Measurements (Phase 8, Release, after final edits)
- **`cargo fmt --check` & `cargo clippy --all-targets -- -D warnings`:** passed (0 warnings).
- **`cargo test --quiet`:** **120 Rust tests pass**, 0 failed, 0 ignored across all workspace crates.
- **Release `xcodebuild` (`Cinma.app`):** passed with **0 compiler warnings** and **0 errors**; `AppIcon.icns` (`1.1 MB`), `en.lproj`, `tr.lproj`, and ad-hoc signature verified in bundle.
- **Launch & Idle (`tools/measure.sh`, Release, 5 runs):**
  - Median first frame: **164.1 ms** (< 250 ms target ✅)
  - Median core ready: **169.1 ms**
  - Median idle `phys_footprint`: **30.0 MB** (≤ 70 MB empty window target ✅), RSS **101.4 MB**
  - Median idle CPU: **0.00 %** (~0 % target ✅)
- **Local playback smoke tests (Release):**
  - 1080p SDR MKV (`sdr_h264_1080p.mkv`): TTFF **391.2 ms**, seeks **85.7 ms** / **142.2 ms**, `gamma=bt.1886 hdr=false`, exit `0`.
  - 4K HDR10 MP4 (`hdr10_hevc_4k.mp4`): TTFF **338.5 ms**, seek **139.1 ms**, `gamma=pq hdr=true`, exit `0`.
- **Visual screenshots (`en` + `tr`):** verified fresh-launch Onboarding (`English (en-US)` vs `Türkçe (tr-TR)` metadata defaults, `39 MB` / `38 MB` footprint), empty Home full-bleed hero scrim, and 4K HDR10 Player HUD contrast via `screencapture` inspection. Home artwork idle (172 MB / 0.17% CPU) and 4K playback CPU (~17%) remain within CLAUDE.md §6 approved targets; Reduce Motion/Reduce Transparency guards (`CinemaTheme.reduceMotion`, `reduceTransparency`) were verified in code (`NSSystem` toggle not flipped interactively via UI).

## Review fixes, round 2 (2026-11-17): everything else from the review
Commits `8c4bda8`…`356b7c6`. 118 Rust tests pass, clippy is clean, and the Release build has no warnings.

- **One process again.** Chromecast discovery is an in-process mDNS query (legacy unicast from an ephemeral port). Packets are parsed with bounds checks and hop-limited name compression, and tested with synthetic packets. CPU detection uses `sysctlbyname`. `yt-dlp -J` is the only child process. Info.plist declares local-network use.
- **Downloads are stored once and land in the user's folders** (ADR 0016):
  - Torrent downloads write into `<folder>/.cinma-partial/<hash>` and are hard-linked into the folder when done, so there is no second copy. Pause detaches the torrent; resume re-checks the pieces on disk.
  - HTTP downloads move their blob into `<folder>/<title>.<ext>`.
  - yt-dlp single tracks reuse the segmented HTTP engine, so they are resumable and playable while downloading.
  - The defaults are `~/Movies/Cinma` and `~/Music/Cinma`; both can be changed in Settings.
  - Downloads has Show in Finder, Open Download Folder, and a Remove dialog that keeps the file or moves it to the Trash. Nothing in a user folder is ever deleted directly.
- **Download durability.**
  - Interrupted downloads resume at launch. Before, they waited until the Downloads queue was touched.
  - Segmented HTTP now writes progress to SQLite at each checkpoint (before: only at the end).
  - Checkpoints run every 10 s instead of 2 s, to cut SSD writes.
  - Every move step is re-doable after a crash (migration 0007, `download.file_path`).
- **Tracking.** The outbox syncs on its own: an event-driven worker sleeps until the next row is due and exits when the outbox is empty. Rows are dropped after 30 failed attempts. The Trakt client secret moved from SQLite (and backups) to Keychain.
- **Restore.** The backup is staged, fsynced and validated, then swapped by rename, with rollback. The previous database is kept as `.pre-restore`. Restore is refused while downloads run.
- **Others.** yt-dlp installs are verified against the release's SHA2-256SUMS. 191 missing Turkish strings are translated, from the compiler's string extraction, not a regex. A flaky timing test is fixed.
- **Verified in the app** (Release, launched with `open -n`):
  - a v6 data dir migrated to v7 and kept its addon;
  - a throttled HTTP download played with Play Now while downloading;
  - after `kill -9` and a relaunch it resumed from 32 MB on its own;
  - it finished as one byte-identical copy: 82 MB in the folder, 236 KB left in the data dir;
  - Remove from List kept the file.

  Torrent single-copy, pause/resume, Play Now across completion, stream-then-download, and yt-dlp single-track placement are covered by loopback/stub tests, not by the UI.
- **Not tested at all:** the yt-dlp mux path (separate video and audio tracks), real devices and services, and a real swarm.
- **Play Now on a torrent download:** the file is moved only after the last viewer closes, so playback never breaks at completion. Starting a download of a torrent that is streaming drops the stream's cache copy, so the data is written once.
- **Tracking worker:** waits at least 60 s after a failed or empty pass, so a row that stays due cannot spin.

### Measured (Release, M-series, 2026-11-17)
| Metric | Value | Target (proposed relaxed values, awaiting owner confirmation) |
|---|---|---|
| Launch, first frame (median of 5) | 169 ms | < 250 ms ✅ |
| Idle footprint, empty window | 30 MB | ≤ 70 MB ✅ |
| Idle, Home with Cinemeta artwork (one 30 s sample) | 172 MB, 0.17 % CPU (hero rotation) | ≤ 200 MB, ≤ 0.5 % ✅ |
| 4K HEVC HDR10 playback (three 20 s samples, one clip) | 16.5 / 17.1 / 17.4 % CPU, ~628 MB | < 20 %, ≤ 700 MB ✅ |

Correction: the Phase 4 note below claims 7.3 % CPU for 4K HDR10. That figure did not reproduce; the measured value is ~17 %, about the same as Phase 1.

### Remaining items (for Phase 8–10 / owner-side tests)
1. **Real-swarm torrent TTFF/seek.** Run `tools/measure-playback.sh <addon manifest url> movie <imdb id> 5` on a normal network (Phase 10 tuning if needed).
2. **Real Chromecast/DLNA and Trakt/Simkl/AniList/MAL round trips.** The code is tested only against local mocks.
3. **Media corpus.** Owner to test Dolby Vision P5/P8/P7, AV1, ASS-heavy and multi-audio samples (`tools/media-corpus.md`). If DV P5 or CPU exceeds limits, enable D1 (`macvk`) with custom `libmpv` in Phase 10.
4. **PiP mini-player (`NSPanel`):** zero-copy always-on-top mini-player selected by owner (`ADR 0011`), scheduled in Phase 8 or Phase 9.
5. **Phase 10 hardening decisions recorded:** test `mimalloc`/`jemalloc` for post-torrent fragmentation (~113 MB), embed minimal `libmpv`+FFmpeg in `Contents/Frameworks` & drop `disable-library-validation`, and remove dormant disk-quota / `keep_watched` code (keep schema tables via migration).

## Review fixes, round 1 (2026-11-17): critical bugs
An owner-requested review of Phases 3–7 found three critical bugs; all three are fixed and verified in the app.
- **First launch stalled on "Loading…"**: onboarding was presented as a sheet in `viewDidLoad`, before the shell had a window, so it opened as a stray off-screen window and the content swap never finished. It is now presented once from `viewDidAppear`. Verified on a fresh data dir: the sheet shows over Home and Skip closes it.
- **Streamed torrents stayed on disk and came back**: the persistent session (fastresume, no delete on release) kept every streamed file and restored it unpaused on the next session. `RqbitEngine` now tracks streaming torrents separately from downloads and deletes them with their files when playback stops (`release_streams`, also when a download keeps the session alive). A `streams.json` marker removes streams left by a crash when the next session starts. Verified in the app over loopback: 72 MB and a persisted torrent before, 4 KB and an empty `session.json` after.
- **Quota could delete finished downloads**: eviction only protected queued/active/paused rows. It now protects every blob a download row owns (`download_blob_ids`), and finished downloads are indexed as pinned.
- Tests: +3 (stream removed on release, crash leftover removed on restart, quota keeps a finished download); 103 Rust tests pass, clippy clean, Release build without warnings.
- The non-critical review items were fixed in round 2 (above).

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
