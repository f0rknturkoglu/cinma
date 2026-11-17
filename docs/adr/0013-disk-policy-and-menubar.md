# ADR 0013 — Disk & seeding policy, memory trim, and menu bar extra

**Status:** proposed (implemented in Phase 5; awaiting owner review)

## 1. Migration `0005_disk_policy.sql`
- `disk_entry`:
  - `entry_key TEXT PRIMARY KEY` (`blob:<id>` or `torrent:<info_hash>`), `kind TEXT NOT NULL CHECK (kind IN ('blob', 'torrent'))`, `meta_id TEXT`, `media_type TEXT`, `path TEXT NOT NULL`, `bytes INTEGER NOT NULL DEFAULT 0`, `last_accessed_at INTEGER NOT NULL DEFAULT (unixepoch())`, `pinned INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1))`.
- `keep_rule`:
  - `(profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE, meta_id TEXT NOT NULL, media_type TEXT NOT NULL, keep_watched INTEGER NOT NULL CHECK (keep_watched IN (0, 1)), PRIMARY KEY (profile_id, meta_id, media_type))`.

## 2. Disk & eviction rules (§5.12)
- Global defaults in `app_setting`: `disk.cache_quota_mb` (default `10240`), `disk.keep_watched` (default `0`), per-type download folders (`disk.folder.movie`, `disk.folder.series`, `disk.folder.ytdlp`, `disk.folder.audio`), `torrent.seeding_mode` (`"off"` | `"ratio"` | `"time"`), `torrent.seeding_ratio` (default `1.0`), `torrent.seeding_minutes` (default `60`), `torrent.upload_limit_kbps` (default `0` = unlimited / disabled when seeding is off).
- Replaces Phase 1's unconditional delete-on-stop for torrents: entries enter `disk_entry`; when total non-pinned size exceeds `disk.cache_quota_mb`, LRU eviction deletes the oldest non-queued, non-pinned (`keep_watched == 0`) blobs and torrent download folders first.

## 3. Post-torrent memory trim & Menu bar extra
- After `Playback::stop()` releases the torrent session (when no torrent downloads remain active), Core triggers `malloc_zone_pressure_relief(NULL, 0)` on macOS (`libc`/Malloc framework) to return freed `librqbit` arena pages promptly.
- In-process `MenuBarController` (`NSStatusItem`, toggled via `menubar.enabled`):
  - When enabled, closing the last window hides it (`applicationShouldTerminateAfterLastWindowClosed` returns `false`), clears `ImagePipeline` caches, and keeps idle menu-bar-only `phys_footprint <= 25 MB` (ADR 0003).
  - While queued/active downloads exist, acquires `NSProcessInfo.processInfo.beginActivity(options: .userInitiatedAllowingIdleSystemSleep, reason: "Cinma active downloads")` to prevent App Nap throttling.
