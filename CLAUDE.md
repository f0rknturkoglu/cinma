# Cinma — standing brief (MASTER PROMPT v3)

> Project name: **Cinma**. Work proceeds phase by phase; report at the end of each phase and wait for confirmation before starting the next.
>
> **Guiding rule:** get a working product on screen first. Build infrastructure only when a real, measured problem demands it.

## Project quick reference

- **Picking this project up? Start with `docs/ROADMAP.md`**: current state, the owner's test checklist, decisions still open, pitfalls already learned, and the next phases (8 UX polish, 9 integrations, 10 hardening).

- Build Rust core + Swift bindings: `tools/build-core.sh [Debug|Release]` (the Xcode "Build Rust core" phase runs it too).
- App: open `app/Cinma.xcodeproj`, scheme **Cinma**. CLI: `xcodebuild -project app/Cinma.xcodeproj -scheme Cinma -configuration Release -derivedDataPath build/xcode build`.
- Rust tests: `cargo test`. Measure: `tools/measure.sh` (appends to `tools/measurements.csv`).
- `app/Generated/` is build output (gitignored). Deployment target lives in `app/Config/Cinma.xcconfig` **and** `tools/build-core.sh` — change both.
- Status: see `PROGRESS.md`. Decisions: `docs/adr/`.

---

## 0. Mission

Build a **personal-use, single-process, native macOS (Apple Silicon) application** that unifies:

1. A **Stremio Addon Protocol client** (catalog, meta, stream, subtitles) with a built-in, hardware-accelerated player.
2. **P2P torrent streaming** with read-cursor-driven piece prioritization (primary use case: Torrentio-style addons returning `infoHash` + `fileIdx`).
3. A **unified download manager**: torrents, multi-segment HTTP, and yt-dlp-based web video/audio — one queue.
4. **Stream-while-downloading**: anything in the queue can be played before it finishes.

The product must beat existing alternatives on **resource usage and core feature completeness at the same time** (see §2).

---

## 1. Non-negotiable principles

If a task seems to require violating one, stop, explain why in a short ADR, and ask.

1. **One process.** No daemons, LaunchAgents, helper apps or sidecar servers. The only tolerated child process is a **short-lived `yt-dlp -J` extraction call** that exits when extraction is done.
2. **No WebView / Electron / Tauri / embedded browser** anywhere.
3. **No localhost HTTP server for local playback.** The player reads bytes from the core through libmpv's custom stream API (`mpv_stream_cb_add_ro`).
   **Single exception — casting:** an in-process LAN HTTP server is started **only while a cast session is active** (Chromecast/DLNA need a URL), and is shut down immediately when casting stops.
4. **Zero telemetry, zero tracking, no mandatory account.** No analytics, no phone-home crash reporters, no remote config. Library, history and settings are local (SQLite). Network calls go only to addons/services the user configured.
5. **Modular features, toggleable at runtime.** Each feature is a plain Rust module/crate behind a clean trait boundary. Disabled features are never instantiated (zero RAM, zero startup cost). **No plugin manifests, permission schemas, plugin API versioning or WASM preparation** — this is a single-developer personal app.
6. **Media bytes stay native.** Feature modules return descriptors ("this infoHash/file", "this URL", "these subtitle tracks"); the data plane (network → BlobStore → player) is core code.
7. **Lazy everything.** libmpv/FFmpeg load on first playback; the torrent session starts with the first torrent; yt-dlp runs only per job; the cast server runs only while casting.
8. **Ship no content sources.** No preinstalled stream addons; the user adds addons by manifest URL or `stremio://` link.
9. **Measure what matters.** Launch time and idle RSS are tracked from day one with a simple script; heavier tooling is added only when a concrete problem needs it.

---

## 2. Competitors to surpass

| Competitor | Strength | Gap we exploit |
|---|---|---|
| Stremio v5 (desktop) | Full addon ecosystem; torrent, Usenet, FTP and archive streaming; libmpv | Web UI shell + separate streaming server; heavy; analytics; no download manager |
| Stremio 2.x (Apple-native app) / Stremio Lite | Native UI, calendar, skip-intro popups | Limited/no torrent without a separate server |
| Flux (SwiftUI/AppKit + libmpv) | Closest native competitor; HDR/DV/AV1; stream racing | No download manager; no published memory figures |
| VortX (stremio-core + libmpv) | Native Apple apps | Bundled streaming server on Mac; sideload signing |
| Harbor (Tauri + Rust) | Very feature-rich: ranking pipeline, skip segments, casting, Trakt, calendar, live TV | WebView UI; torrents via bundled Stremio Server; yt-dlp-only downloads |
| Stremio Enhanced / Raffi / Rillio | Customization | Electron/Tauri + sidecar servers |
| Downie / Pulltube | Polished native yt-dlp downloaders | No torrent, no general HTTP, no player |
| Gopeed, Motrix(Next), AB DM, JDownloader | HTTP + torrent | Flutter/Electron/JVM runtimes; no real player; no stream-while-downloading |
| qBittorrent / Transmission | Mature torrenting | No player, no addons |
| IINA / Infuse | Player quality bar | No addon protocol, no torrent, no downloads |

**Bar:** Flux-level native playback + Downie-level download UX + Harbor-level stream selection quality, in one process, with lower idle RAM than any of them.

---

## 3. Fixed tech stack

- **UI:** Swift 6 + **AppKit** (`NSCollectionView` for poster grids, `NSTableView` + diffable data sources for queues). SwiftUI only for leaf views (settings, small panels). No SwiftUI `App` lifecycle in the launch path.
- **Core:** **Rust**, `tokio` with a small bounded worker pool, exposed via **UniFFI** (async functions + callback interfaces). Only one FFI crate is visible to Swift.
- **Player:** **libmpv**, custom-built: minimal FFmpeg (only needed demuxers/decoders), libplacebo, libass, VideoToolbox hwdec; loaded lazily on first playback.
- **Torrent:** **librqbit**, behind a `TorrentEngine` trait. Only if librqbit demonstrably fails the streaming targets in real use, evaluate libtorrent-rasterbar (write an ADR first; do not pre-build the adapter).
- **HTTP:** `reqwest`/`hyper` + `rustls`; own segmented download engine.
- **Web video:** `yt-dlp -J` for extraction; our HTTP engine downloads; in-process **libavformat** muxes.
- **Database:** **SQLite** via `rusqlite`, WAL, `STRICT` tables, versioned migrations. No Core Data / SwiftData.
- **Secrets:** macOS **Keychain**.
- **Localization:** Xcode **String Catalogs** from day one (English + Turkish).
- **Metadata:** **TMDB** (user's own free API key, asked in onboarding) as the recommended primary metadata/artwork source; **Cinemeta** as the keyless fallback. Optional Fanart.tv for logos/HD art.
- **Visual layer:** AppKit + Core Animation, **`NSGlassEffectView`** (macOS 26 Liquid Glass) for controls, with an `NSVisualEffectView` fallback on older macOS.

---

## 4. Repository layout (target)

```
/app                    Xcode project (Swift/AppKit)
/crates
  core/                 runtime, settings, lifecycle, event stream to UI, command registry
  blobstore/            sparse files + availability bitmaps + composable ByteSource
  queue/                unified download queue, priorities, bandwidth limits
  db/                   SQLite schema, migrations, repositories
  ffi/                  UniFFI surface (the ONLY crate Swift sees)
  player-bridge/        libmpv stream_cb glue, thumbnail decoder
  features/
    stremio/            Stremio Addon Protocol client
    torrent/            TorrentEngine (librqbit)
    http/               multi-segment HTTP engine
    ytdlp/              extraction, format selection, mux
    ranker/             stream parsing & scoring
    subtitles/          addon subtitles + OpenSubtitles direct
    cast/               Chromecast/DLNA + on-demand LAN server   (Phase 6)
    sync/               Trakt, Letterboxd                         (Phase 6)
/tools
  measure.sh            launch time + idle RSS + CPU snapshot
  media-corpus.md       list of test files (HDR10, DV P5/P8, ASS-heavy, multi-audio, AV1, HEVC, MKV cues-at-end, MP4 moov-at-end)
/docs/adr               short architecture decision records
PROGRESS.md             phase, done, next, risks
```

---

## 5. Architecture specification

### 5.1 BlobStore
- Every media object (torrent file, HTTP download, yt-dlp progressive download) is a **Blob**: sparse file + range availability bitmap + metadata (size, source, keep/evict).
- Engines write into Blobs; the player and thumbnail decoder read from them.
- `ByteSource::read(offset, len)` blocks until the range is available or cancelled; every read emits a **priority signal** (cursor + read-ahead window) to the owning engine.
- **Head and tail first** (MKV cues / MP4 `moov` can be at the end; OpenSubtitles hash needs first/last 64 KB).
- **`ByteSource` is composable**: a source can wrap another source. This is the slot for future **archive playback** (store-mode RAR/ZIP inside torrents) — design for it now, do not implement yet.

### 5.2 Player bridge
- Custom protocol (e.g. `nx://blob/<id>`) via `mpv_stream_cb_add_ro` with `read`, `seek`, `size`, `close` and **`cancel`** (seeks/stops must never hang).
- Plain HTTP(S) streams may be opened by mpv directly unless the user chose to keep the file.
- **Source switching is first-class:** the player can swap the underlying stream mid-playback (same position), used by the stream switcher and by auto-recovery (stall/black-screen → retry → next ranked stream).
- Render path: decide in Phase 1 between libmpv render API on `CAOpenGLLayer` with EDR (proven) and `gpu-next` via MoltenVK (`macvk`). Short ADR with measurements.
- Required: VideoToolbox hwdec, HDR10/HLG, Dolby Vision (P5/P8 via libplacebo; document P7 limits), MKV, multi-audio, ASS/SSA via libass, external subs, **dual subtitles** (`secondary-sid`), audio/sub delay, chapters, speed.
- **Seek-bar thumbnails:** in-process keyframe decoder (libavformat + VideoToolbox, ~160 px, LRU). Precompute at low QoS for complete local files; for streams use only already-available ranges — **never fetch data just for thumbnails**.
- macOS: `MPNowPlayingInfoCenter` + media keys, power assertion during playback.

### 5.3 Torrent
- Streaming: sliding priority window around the read cursor; head+tail first; trackers from addon `sources` + DHT.
- Download mode: full completion, file selection in multi-file torrents.
- Lazy session, fastresume, rate limits, seeding policy from settings.

### 5.4 Segmented HTTP
- Dynamic segmentation (split the largest remaining segment when a connection frees); per-host cap.
- **HTTP/1.1 for multi-segment transfers** (HTTP/2 multiplexing defeats parallel connections).
- Resume validated with `ETag`/`Last-Modified`.
- When a downloading file is played, open a segment at the read cursor.

### 5.5 Features & toggles
- Plain traits where they earn their keep: `StreamSource`, `StreamRanker`, `SubtitleProvider`, `SyncAdapter`, `TorrentEngine`.
- A settings table holds on/off per feature; disabled features are never constructed.
- Fixed flow (no event bus): `catalog → meta → streams → rank → play → subtitles → progress → finished`.

### 5.6 Stream ranking
- Collect from all addons **in parallel**, show partial results as addons answer.
- Parse `name`/`title`/`behaviorHints` (`filename`, `videoSize`, `bingeGroup`): resolution, HDR/DV, codec, audio, size, seeders, release group, language.
- Pipeline: **parse → filter (fakes, CAM/TS, safety level) → score → tier**. Scores consider the machine's capabilities (e.g. hardware AV1 only on newer chips) and user preferences.
- Explainable: the UI can show why a stream ranked where it did.

### 5.7 yt-dlp
- `yt-dlp -J` as a short-lived child; select formats; download with our HTTP engine; mux with libavformat.
- yt-dlp binary updated only when the user triggers it.
- Also used for **trailers** in the detail view.

### 5.8 Subtitles
- Default behavior **like Stremio**: query subtitle addons with `videoHash`, `videoSize`, `filename`.
- Optional direct **OpenSubtitles** provider (user key/login in Keychain).
- Compute the OpenSubtitles hash as soon as head/tail ranges arrive.
- Language-priority auto-select; dual-subtitle presets; **encoding detection** (must handle Turkish `windows-1254` / `ISO-8859-9`); per-file timing offset; SRT style overrides; custom font loading; respect SDH/forced flags. Never restyle ASS unless asked.

### 5.9 Next episode
- Resolve next episode via `bingeGroup` + series meta.
- At ~90% (or credits chapter), prefetch next episode's meta, ranked streams and head/tail pieces of the best candidate; cancel if playback stops.
- Configurable auto-play countdown; skip intro/outro via chapters.

### 5.10 Library & data model
- SQLite is the single source of truth: library, progress, watched state, downloads, settings, TTL'd meta cache.
- **Every user-data table carries `profile_id` from day one.** Per profile: library, progress, watched state, subtitle/audio language preferences, "keep watched" setting, tracking-service connections, avatar. Addons are installed globally and enabled per profile. Optional PIN per profile.
- Backup/restore = export/import of the SQLite database + settings.

### 5.11 Command registry
- All user actions (menu items, hotkeys, deep links like `stremio://`) go through one command registry, so **hotkey remapping** and deep links are configuration, not new code.

### 5.12 Disk policy (Settings)
- "Keep watched content" toggle (global + per item).
- Streaming cache quota with LRU eviction of non-kept Blobs; never evict queued or kept items.
- Seeding policy (ratio / time / off); default folders per type.

### 5.13 Metadata & ID mapping
- A local **ID map table** links IMDb, TMDB, TVDB, Trakt, Simkl, MAL, AniList and Kitsu IDs for movies, shows and episodes. Every metadata source and tracking adapter resolves IDs through it; no adapter solves ID mapping on its own.
- Populate from TMDB `external_ids`, Trakt/Simkl `ids` objects and an open anime ID-mapping dataset (bundled, user-refreshable).
- TMDB provides: backdrops, posters, logos, localized (Turkish) titles and overviews, cast/crew, person pages, trailers, air dates (calendar), region-aware watch providers. Cache responses with TTL; images go through the image pipeline (§5.15).

### 5.14 Tracking services (all `SyncAdapter`s, all off by default)
- Shared **outbox** in SQLite: every watch event writes a local record + one outbox row per enabled adapter; a worker drains with retry/backoff. Offline-safe; enabling an adapter later backfills history.
- Adapters: **Trakt** (device-code OAuth, scrobble, history, watchlist, recommendations), **Simkl**, **AniList** and **MyAnimeList** (anime, via the ID map), **Letterboxd** (RSS read + CSV export in its import format; no scraping).
- Tokens in Keychain, per profile.

### 5.15 Visual design — "dark cinema + Liquid Glass"
- **Surfaces: cinematic and dark.** Near-black base (not pure black on non-OLED; use a warm-neutral ~#0B0B0D), content-driven color: full-bleed **backdrops** with soft bottom/left gradient scrims, title **logos** instead of text where available, generous negative space. The artwork is the hero; chrome stays quiet.
- **Controls: Liquid Glass.** Buttons, toolbars, segmented controls, the player control bar, popovers and the stream list sheet use `NSGlassEffectView` (Regular for bars, Clear over artwork), so they float over the backdrop and pick up its color.
- **Typography:** SF Pro Display for titles (tight tracking, large sizes on hero), SF Pro Text for body; numbers (runtime, size, seeders) in monospaced digits.
- **Accent:** a single warm accent (amber/gold) for progress bars, focus rings and the primary "Play" action. Quality badges (4K, DV, HDR10, Atmos) as small glass chips.
- **Screens to design first:** Home (rotating hero + rails + Continue Watching), Title detail (backdrop, logo, metadata, seasons/episodes), Stream picker (ranked list with tier badges + "why"), Player controls (glass bar, track menus, thumbnail scrubber, next-episode card), Downloads queue, Menu bar panel, Profile picker, Onboarding (addons + TMDB key).
- **Motion:** Core Animation only; subtle parallax on hero, crossfades between backdrops, spring transitions for sheets. Must hold 120 Hz on ProMotion; respect Reduce Motion and Reduce Transparency (glass falls back to solid dark surfaces).
- **Image pipeline:** ImageIO downsampling to display size, progressive load (blurred placeholder → full), memory `NSCache` with cost limits + capped disk cache.
- Mockups produced in the design phase are the visual reference; match them.

### 5.16 Memory & launch discipline
- Launch: AppKit window → cached home rows from SQLite → draw → network (stale-while-revalidate).
- Images: ImageIO downsampling to display size; `NSCache` with cost limits; capped disk cache.
- Destroy the mpv handle when playback ends; drop image caches when the window closes and the queue is idle.
- Downloads continue via a **menu bar extra in the same process**; manage App Nap with `NSProcessInfo` activities.

---

## 6. Targets (Apple Silicon M1 baseline)

On 2026-11-17 the owner said RAM and CPU targets may be loosened a little, without giving numbers. The **new values in this table are proposed by the agent and await the owner's confirmation**. The old value is in brackets where it changed.

| Metric | Target | Last measured |
|---|---|---|
| Cold start → first interactive frame | **< 250 ms** | 169 ms |
| Idle footprint, window open, empty | **≤ 70 MB** | 30 MB |
| Idle footprint, Home with artwork loaded | **≤ 200 MB** (new) | 172 MB |
| Idle footprint, menu bar only, queue idle | ≤ 40 MB | 28 MB (previous agent; not re-measured) |
| Idle CPU | **≤ 0.5 %** with Home visible (hero rotation); ~0 % with the window closed [~0 %] | 0.17 % / 0.00 % |
| 4K HEVC HDR10 playback CPU (hwdec) | **< 20 % of one core** [< 10 %] | ~17 % |
| Playback footprint, 4K HDR | **≤ 700 MB** (new) | ~630 MB |
| Torrent time-to-first-frame (healthy real swarm) | median < 5 s | not measured (sandbox) |
| Seek recovery (healthy swarm) | median < 2 s | not measured (sandbox) |
| Thumbnail hover | < 16 ms cached, < 120 ms on-demand local | — |

`tools/measure.sh` tracks launch time and RSS from Phase 0. Competitor comparisons are done manually with the same script at milestones. If torrent behavior later needs reproducible regression testing, a local test swarm may be introduced then (ADR first).

---

## 7. Phases

Each phase ends with a short report (built, measured vs targets, risks) and green tests.

### Phase 0 — Skeleton (days, not weeks)
- Rust workspace + Xcode project + UniFFI wiring + SQLite migrations scaffold + String Catalogs.
- `tools/measure.sh`.
- **Exit:** empty AppKit window launches < 150 ms; baseline RSS recorded.

### Phase 1 — Vertical slice: a torrent playing on screen
- Add an addon by URL → fetch streams for one title → pick a Torrentio-style stream → librqbit → BlobStore → `mpv_stream_cb` → video playing in the window, with seeking.
- Decide the render path (EDR/OpenGL vs macvk) on a few HDR/DV files.
- **Exit:** real torrent plays and seeks reliably; TTFF and seek measured; render-path ADR written.

### Phase 2 — Core solid
- BlobStore (cancellation, head/tail priority, composable `ByteSource`), unified queue skeleton, DB schema with `profile_id`, settings + feature toggles, command registry, event stream to UI.
- Crash-safety test: kill -9 mid-write → consistent state on restart.
- **Exit:** core tests green; app still meets launch/RSS targets.

### Phase 3 — Browsing & stream selection
- Addon manager (URL / `stremio://` install, enable/disable, order, open addon config page), catalogs, search, detail pages (series/seasons/episodes), library, continue watching, **calendar** (upcoming episodes from library), trailers via yt-dlp.
- Ranker per §5.6 with stream list filters and "why this stream".
- TMDB metadata + ID map (§5.13), onboarding asks for the TMDB key (Cinemeta fallback if skipped).
- Visual design per §5.15 and the mockups; profile picker + per-profile data (schema already supports it).
- **Exit:** browse → choose → play end-to-end through the real UI.

### Phase 4 — Player complete
- Everything in §5.2, subtitles §5.8, next episode §5.9, thumbnails, resume.
- Player extras: stream switcher, auto-recovery, A/B loop, sleep timer, screenshot, stats overlay, shader-based upscaling option, HDR target-peak setting, hotkey remapping (via command registry).
- **PiP spike:** macOS PiP is not directly compatible with libmpv rendering — investigate feasibility, report before building.
- **Exit:** media-corpus playback checklist passes; CPU and thumbnail targets met.

### Phase 5 — Unified download manager
- Torrent download mode, segmented HTTP, yt-dlp jobs (formats, audio-only, mux), unified queue UI, **Play now** for every item type, disk policy §5.12, menu bar extra.
- **Exit:** any queued item plays before completion; kill -9 mid-download resumes correctly; idle targets met with an idle queue.

### Phase 6 — Integrations (each off by default, zero cost when off)
- **Casting:** Chromecast + DLNA with the on-demand in-process LAN server (§1.3). AirPlay: investigate what's feasible with libmpv; report.
- **Tracking adapters** per §5.14: Trakt first, then Simkl, AniList, MyAnimeList, Letterboxd.
- One-time **Stremio account import** (addon list, library) — optional.
- **Exit:** each integration toggles cleanly; RSS/launch unchanged when off.

### Phase 7 — Polish
- Error UX (dead streams, no peers, addon down), onboarding polish, accessibility (VoiceOver labels, full keyboard navigation), profile PINs, backup/restore UI, packaging/signing.
- **Exit:** all §6 targets met; manual comparison vs Stremio v5 / Flux / IINA documented.

### Phase 8 — UI/UX polish · Phase 9 — Integrations & add-ons · Phase 10 — Hardening & release
Added after Phases 0–7 were built and reviewed (2026-11-17). Task lists, priorities and exit criteria are in `docs/ROADMAP.md` §7–9.

### Backlog (not scheduled; architecture slots already exist)
- Live TV / IPTV module (M3U, Xtream, XMLTV/EPG).
- Archive playback (store-mode RAR/ZIP) via composable `ByteSource`.
- Skip-segment databases (AniSkip / IntroDB), anime metadata (Kitsu).
- VPN interface binding / kill switch for torrent traffic.
- Debrid (Torrentio's debrid mode already works as plain HTTP links).
- Discord Rich Presence, watch party, multiview, parental controls, native themes.
- **Out of scope:** Usenet.

---

## 8. Working rules for the agent

- Read this file at session start; keep `PROGRESS.md` and short ADRs current.
- Small commits, tests with each feature; no `unwrap()` on I/O/network paths.
- **New dependency = one-paragraph justification** (size, maintenance, license).
- Never introduce: background processes, WebViews, telemetry, idle polling, unbounded caches.
- **Don't build infrastructure ahead of need.** If you think a tool, abstraction or adapter is needed, write a 5-line ADR and ask.
- Ask before irreversible or ambiguous decisions (data formats, engine changes).
- Keep business logic in Rust; Swift is presentation only.
