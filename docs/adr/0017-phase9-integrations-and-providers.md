# ADR 0017 — Phase 9 Integrations, Resolvers, Live TV, Archive Streaming & Discord IPC

**Status:** proposed (Phase 9 implementation, 2026-11-17)

## Context
`docs/ROADMAP.md` §8 defines Priorities 1–12 for Phase 9. Each new subsystem must honor `CLAUDE.md` §1 (one process, zero WebViews, zero telemetry, lazy initialization, off-by-default toggles with zero idle RAM/CPU when disabled) and `CLAUDE.md` §8 (no unnecessary dependencies).

## Decisions
1. **Runtime feature boundaries (`crates/core/src/settings.rs`):**
   - Add `Feature::SkipSegments`, `Feature::Debrid`, `Feature::Iptv`, and `Feature::Discord`, all **off by default**. Disabled features never construct HTTP clients, Unix sockets, or playlist/segment state and tear down active state immediately when toggled off.
2. **Zero new external Cargo dependencies:**
   - Reuse workspace crates (`reqwest`, `serde`, `serde_json`, `url`, `ring` for PKCE SHA-256, `tokio::net::UnixStream` for Discord IPC, `encoding_rs` + `rusqlite`) for all new crates (`crates/features/debrid`, `crates/features/iptv`, `crates/features/discord`, plus extensions to `cinma-metadata`, `cinma-tracking`, `cinma-subtitles`, `cinma-ranker`, `cinma-blobstore`, `cinma-torrent`).
3. **Skip segments & Metadata enrichment (`cinma-metadata`, `0008_phase9_integrations.sql`):**
   - Query AniSkip v2 (`mal_id`) and IntroDB (`imdb_id`, season, episode) via `id_map`, cache results in `skip_segment_cache` (7-day TTL), and merge them into the player's existing Skip button alongside chapter markers.
   - Add optional Fanart.tv v3 (`clearlogo`, extra backdrops with user Keychain key), TMDB watch providers per metadata country, and Kitsu edge API anime fallback; extend `anime_map` to parse upstream Fribb anime-lists datasets.
4. **Debrid resolvers (`crates/features/debrid`) & Ranker:**
   - Support Real-Debrid, AllDebrid, Premiumize, and TorBox via per-profile Keychain keys. Batch-check `info_hash` instant availability, annotate `StreamCandidate.debrid_cached`, boost cached streams in `cinma-ranker`, and unrestrict into direct HTTPS URLs on play/download (with P2P torrent fallback if resolution fails).
5. **OAuth PKCE, Two-Way Trakt & OpenSubtitles Account (`cinma-tracking`, `cinma-subtitles`):**
   - Register custom URL scheme `cinma://oauth/<provider>` in `Info.plist` so Simkl, AniList, and MAL open in the user's system browser (`NSWorkspace.shared.open`) and return via PKCE/OAuth callback—no WebView.
   - Extend Trakt with `/sync/watchlist`, `/sync/ratings`, and `/sync/history` pull merged into `library_item` / `watch_progress` via `id_map`.
   - Add OpenSubtitles JWT `/login`, `/infos/user` quota inspection, and in-player hash search sheet.
6. **Store-mode Archive `ByteSource` (`crates/blobstore/src/archive.rs`):**
   - Implement `ArchiveByteSource: ByteSource` parsing ZIP EOCD/Central Directory (`method == 0`) and RAR4/RAR5 store headers (`0x30`) over any outer `ByteSource`, forwarding `read_at`/`try_read_at` and priority cursor hints directly to torrent/file sources.
7. **Live TV / IPTV (`crates/features/iptv`), Calendar Notifications, VPN Binding & Discord:**
   - Parse M3U/M3U8, Xtream Codes JSON, and XMLTV EPG (`0008_phase9_integrations.sql`); play live channels through `PlayerViewController` / `libmpv`.
   - Schedule upcoming episode alerts locally via `UserNotifications` (`UNUserNotificationCenter`, off by default, zero background daemon).
   - Expose `librqbit::SessionOptions::bind_device_name` + fail-closed VPN kill switch (enumerating interfaces via Darwin `getifaddrs` in Swift — zero `unsafe` in Rust and zero helper processes).
   - Speak Discord's local `$TMPDIR/discord-ipc-{0..9}` Unix socket RPC in-process. Optional `AppIntents` / `CoreSpotlight` index library items lazily with zero launch-path overhead.

## Consequences
- Zero external Cargo dependencies added; all new network integrations are mock-tested locally (`tokio::net::TcpListener` / Unix domain socket tests) and real-service round trips stay on the owner's test checklist.
