# ADR 0014 — Casting (Chromecast + DLNA) and Tracking Outbox + Profile PIN KDF

**Status:** proposed (implemented in Phases 6–7; awaiting owner review)

## 1. On-demand LAN HTTP server for Casting (`crates/features/casting`)
- No persistent HTTP daemon is ever started (`CLAUDE.md` §1.3). An in-process `tokio::net::TcpListener` (`http1`) spins up **only** while an active cast session runs, binds to the LAN interface with an ephemeral port and a per-session random URL path token, serves HTTP `Range` requests (`206 Partial Content`) over an `Arc<dyn ByteSource>` or local file, and shuts down immediately on `stop()`.
- **Discovery & Control:**
  - **Chromecast:** `_googlecast._tcp` mDNS discovery via macOS `dns-sd` / `NWBrowser`, plus CastV2 TLS (`port 8009`, protobuf framing for `CONNECT`, `LAUNCH` Default Media Receiver `CC1AD845`, `LOAD`, `PLAY`, `PAUSE`, `SEEK`, `STOP`).
  - **DLNA:** SSDP `M-SEARCH` multicast (`239.255.255.250:1900`, `urn:schemas-upnp-org:device:MediaRenderer:1`) + UPnP `AVTransport:1` SOAP actions (`SetAVTransportURI`, `Play`, `Pause`, `Seek`, `Stop`).
  - **AirPlay feasibility:** System AirPlay video streaming on macOS requires `AVPlayer`; `vo=libmpv` OpenGL cannot stream directly over AirPlay Video/HAP without system Screen Mirroring (audio output already routes to AirPlay speakers through CoreAudio).

## 2. Tracking Outbox & Adapters (`crates/features/tracking`, `0006_tracking_outbox.sql`)
- Every completed/watched update appends outbox rows in SQLite (`sync_outbox`) for each enabled adapter (`trakt`, `simkl`, `anilist`, `mal`); a background worker drains due rows with exponential backoff (`30s`, `2m`, `10m`, `1h`).
- Enabling an adapter offers one-click history backfill from `watch_progress` + `library_item` resolved via `id_map`.
- **Letterboxd:** Read-only public RSS parser (`/user/rss/`) + official CSV export (`Title,Year,WatchedDate,Rating10,Review`) — zero scraping.
- Per-profile OAuth tokens live in macOS Keychain (`app.cinma.mac`, `tracking.p<id>.<adapter>`).

## 3. Profile PIN KDF (Phase 7)
- `profile.pin_hash` (`0002_core_schema.sql`) uses `pbkdf2$<16-byte hex salt>$<32-byte hex DK>` computed with `ring::pbkdf2` (`PBKDF2_HMAC_SHA256`, 100,000 iterations) and verified in constant time via `ring::constant_time::verify_slices_are_equal`.

## Update (2026-11-17, review round 2)
- **Chromecast discovery** no longer spawns `/usr/bin/dns-sd`, which broke §1 "one process". It is an in-process one-shot mDNS query (`cinma-casting::mdns`): legacy unicast from an ephemeral port, nothing bound to 5353, the socket dropped afterwards, packets parsed with bounds checks.
- **Outbox:** the worker now runs automatically. It is event-driven: it starts on enqueue, on enabling tracking or a token, and at launch if rows are waiting. It sleeps until the next due row and exits when the outbox is empty. Rows are dropped after 30 attempts or on permanent errors.
- **Trakt client secret:** stored in Keychain, not in `app_setting`.
