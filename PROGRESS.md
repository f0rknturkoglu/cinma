# PROGRESS

## Current phase: 1 — Vertical slice (built; awaiting owner review)

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
  - render API on `CAOpenGLLayer` with EDR/HDR switching (ADR 0004).
- **App**: Phase-1 screen (install addon → find streams → double-click to play, plus Open File…) and a player with a control bar (space, ←/→ ±10 s, Esc). English and Turkish strings.
- **Tools**: `tools/measure-playback.sh` (TTFF and seek through the real app); env-driven dev automation (`DevAutomation.swift`); a `local_seed` example.

### Measured
| Metric | Value | Target |
|---|---|---|
| Launch, first frame (median of 7) | 158 ms | < 250 ms ✅ (floor ~152 ms) |
| Idle footprint, window open | 25 MB | ≤ 40 MB ✅ |
| Idle CPU, before and after playback | ~0 % | ~0 % ✅ |
| TTFF, loopback seeder at 6 MB/s, from play click (median of 5) | 237 ms | — |
| Seek into undownloaded region, loopback 6 MB/s (median) | 979 ms | — |
| **TTFF / seek on a real swarm** | **not measured** | < 5 s / < 2 s |
| 4K HDR10 playback CPU | ~19 % | < 10 % ❌ (Phase 4) |
| Footprint after a torrent playback ends | ~113 MB | ≤ 40 MB ❌ (Phase 2) |

Real-swarm numbers are missing because the agent's sandbox proxies outbound TCP, so it can't reach BitTorrent peers. To measure on a normal network:
`tools/measure-playback.sh <addon manifest url> movie <imdb id> 5`

### Risks / open items
1. **Real-swarm TTFF and seek** are unverified. librqbit streams with a 32 MB window ahead of the cursor and prioritizes the first and last pieces. Whether that meets < 5 s / < 2 s on real swarms is unknown.
2. **Memory after torrent playback**: footprint stays at ~113 MB, while the live heap is only 13.5 MB. The rest is malloc fragmentation in the small zone; `malloc_zone_pressure_relief` frees ~0. Phase 2 candidates: mimalloc as the global allocator, or bounding librqbit's buffers (ADR first).
3. **Playback CPU** is ~20 %. **Playback footprint** is 470–630 MB. Both are Phase 4 (ADR 0004).
4. **OpenGL render path** is deprecated. **Homebrew libmpv** is a stand-in until a custom build (ADRs 0004, 0005).
5. Rendering happens on the main thread. A long `mpv_render_context_render` would stall the UI. Move it to a dedicated thread in Phase 4.

### Next: Phase 2 — Core solid
BlobStore (sparse files, bitmaps, head/tail, composable sources), unified queue skeleton, DB schema with `profile_id` (addon tables included), settings and feature toggles, command registry, event stream, kill -9 test.

---

## Phase 0 — Skeleton ✅ (accepted 2026-11-17)
Rust workspace, UniFFI wiring, SQLite migrations, AppKit shell, String Catalog, `tools/measure.sh`. Launch 150–160 ms, matching the bare-AppKit floor of ~152 ms. Idle footprint 20 MB. ADRs 0001–0003.
