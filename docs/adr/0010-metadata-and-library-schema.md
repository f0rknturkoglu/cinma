# ADR 0010 — Metadata cache, ID map, library schema, Keychain, and yt-dlp source

**Status:** accepted (owner, 2026-11-17)

Following ADR 0008 conventions (`STRICT` tables, unix-second timestamps, `profile_id` foreign keys on all user-data tables, raw JSON payloads where appropriate), Phase 3 steps 3, 4, and 11 introduce two schema migrations and two integration choices.

## Migration 0003 — `id_map` and `meta_cache`

- `id_map`:
  - Columns: `id INTEGER PRIMARY KEY`, `media_kind TEXT NOT NULL CHECK (media_kind IN ('movie', 'series', 'episode'))`, `imdb_id TEXT`, `tmdb_id INTEGER`, `tvdb_id INTEGER`, `trakt_id INTEGER`, `simkl_id INTEGER`, `mal_id INTEGER`, `anilist_id INTEGER`, `kitsu_id INTEGER`, `season INTEGER`, `episode INTEGER`, `updated_at INTEGER NOT NULL DEFAULT (unixepoch())`.
  - Partial indexes on `(media_kind, imdb_id)`, `(media_kind, tmdb_id)`, `(media_kind, tvdb_id)`, `(media_kind, trakt_id)`, `(media_kind, simkl_id)`, `(media_kind, mal_id)`, `(media_kind, anilist_id)`, `(media_kind, kitsu_id)` (where the respective column `IS NOT NULL`) for fast cross-provider lookup.
- `meta_cache`:
  - Columns: `cache_key TEXT PRIMARY KEY` (e.g. `tmdb:movie:550:tr-TR` or `cinemeta:series:tt0944947`), `source TEXT NOT NULL`, `json TEXT NOT NULL`, `expires_at INTEGER NOT NULL`, `updated_at INTEGER NOT NULL DEFAULT (unixepoch())`.
  - Expired rows are ignored on read and pruned opportunistically so cache size stays bounded.

## Migration 0004 — `library_item` and `watch_progress` (profile-scoped)

- `library_item`:
  - Columns: `profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE`, `meta_id TEXT NOT NULL`, `media_type TEXT NOT NULL`, `name TEXT NOT NULL`, `poster TEXT`, `added_at INTEGER NOT NULL DEFAULT (unixepoch())`, `PRIMARY KEY (profile_id, meta_id, media_type)`.
- `watch_progress`:
  - Columns: `profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE`, `video_id TEXT NOT NULL`, `meta_id TEXT NOT NULL`, `media_type TEXT NOT NULL`, `position_s REAL NOT NULL`, `duration_s REAL NOT NULL`, `watched INTEGER NOT NULL DEFAULT 0 CHECK (watched IN (0, 1))`, `updated_at INTEGER NOT NULL DEFAULT (unixepoch())`, `PRIMARY KEY (profile_id, video_id)`.
  - Index on `(profile_id, watched, updated_at DESC)` to serve the "Continue watching" rail directly from SQLite at launch before any network call (§5.16).

## TMDB API key in Keychain

- Store the user's TMDB API key in macOS Keychain under service `app.cinma.mac`, account `tmdb.api_key`.
- Two implementation options:
  1. **Swift `Security` wrapper** (zero new Rust dependencies; `Security.framework` is already linked by Xcode) reads the key when needed and passes it to `Core`.
  2. **Rust `security-framework` crate** (already in `Cargo.lock` via `reqwest`/`rustls-platform-verifier`) reads/writes Keychain directly inside `crates/core`.

## `yt-dlp` binary location (Phase 3 step 11 — trailers)

- Lookup order: `CINMA_YTDLP` env override → `~/Library/Application Support/Cinma/bin/yt-dlp` → `/opt/homebrew/bin/yt-dlp` (same Homebrew-first pattern as ADR 0005 for `libmpv`).
- Never auto-downloaded at launch or in the background; an explicit "Install / Update yt-dlp" button in Settings/Onboarding downloads the official macOS binary only when the user triggers it (§5.7).
