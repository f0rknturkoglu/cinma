# ADR 0002 — Phase 0 defaults (cheap to change now)

**Status:** accepted (owner, 2026-11-17)

| Decision | Choice | Why |
|---|---|---|
| Minimum macOS | 15.0 | Brief asks for an `NSVisualEffectView` fallback below macOS 26, so <26 must build. Raise to 26 to drop the fallback. |
| Architecture | arm64 only | Apple Silicon is the baseline; halves Rust build time. |
| Bundle ID | `app.cinma.mac` | Neutral placeholder. |
| Signing | ad-hoc (`-`), no hardened runtime | Personal app, runs locally. Revisit in Phase 7 (packaging). |
| App Sandbox | off | Arbitrary download folders, `yt-dlp` child process, torrent sockets. Sandboxing would need security-scoped bookmarks + entitlements; revisit in Phase 7. |
| SQLite | system `libsqlite3` (not bundled) | Already in the dyld shared cache → no extra binary size, less dirty memory. Supports WAL + `STRICT`. Switch to `bundled` if we need a specific SQLite version/extension. |

## Dependencies (Phase 0)

- **uniffi 0.32** (MPL-2.0, Mozilla, very active) — the FFI layer mandated by the brief.
- **rusqlite 0.43** (MIT, de-facto standard SQLite binding, active) — the DB layer mandated by the brief; no `bundled` feature (see above).
- **thiserror 2** (MIT/Apache-2.0, dtolnay, tiny proc-macro, zero runtime cost) — typed errors without boilerplate.
- **tempfile 3** (dev-only, MIT/Apache-2.0) — isolated directories in tests.
- Migrations are a ~20-line runner over `PRAGMA user_version` instead of a migrations crate.
