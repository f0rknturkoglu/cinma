# PROGRESS

## Current phase: 2 — Core solid (built; awaiting owner review)

### Done in Phase 2
- **Schema 0002** (ADR 0008, *proposed*): `addon` + `profile_addon` (installed globally, enabled per profile), `profile_setting`, `download`; profile gets `avatar` and `pin_hash`.
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
| Tests | 61 Rust tests (incl. 2 kill -9 suites), clippy clean, Swift build warning-free | — |

### Re-verified after the refactor
- Torrent slice end to end on loopback: TTFF 205 ms, seek 1.3 s; streamed data cleared on close.
- Run on a copy of a data dir written by the Phase 1 build: v1 → v2 migration kept the addon and enabled it for profile 1.
- Debug build: launches and plays a torrent; Info.plist merge OK.

### Open decisions for the owner
1. ADR 0008 schema (proposed).
2. ADR 0009: torrents stay on librqbit storage, outside BlobStore (proposed).

### Next: Phase 3 — Browsing & stream selection

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
