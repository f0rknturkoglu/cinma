# ADR 0015 — Packaging, Custom libmpv Bundle Precedence, and SQLite Backup

**Status:** partially accepted (owner, 2026-11-17: §1 `libmpv` lookup order and §2 backup/restore accepted; packaging/signing to be rewritten in Phase 10 with minimal bundled `libmpv`+FFmpeg, dropping `disable-library-validation`)

## 1. `libmpv` Lookup Precedence (ADR 0005 completion)
- `crates/player-bridge/src/sys.rs` checks `CINMA_LIBMPV` → `@executable_path/../Frameworks/libmpv.2.dylib` (resolved via `std::env::current_exe()`) → `/opt/homebrew/lib/libmpv.2.dylib` → `/usr/local/lib/libmpv.2.dylib`.
- Distributable builds bundle a custom minimal LGPL `libmpv.2.dylib` (VideoToolbox + minimal FFmpeg + libass + libplacebo, enabling D1 `macvk` Dolby Vision P5 per ADR 0004) in `Cinma.app/Contents/Frameworks/`, while local dev builds continue falling back cleanly to Homebrew `libmpv`.
- `tools/package-release.sh` prepares Release app bundles, entitlement signing (`com.apple.security.network.client`, `com.apple.security.files.user-selected.read-write`, `com.apple.security.device.audio-output` / local network usage description), and optional custom `Frameworks/` embedding + `codesign --options runtime`.

## 2. Online SQLite Backup / Restore
- Exports use SQLite's native `VACUUM INTO <dest_path>` inside `cinma-db` (transactional, WAL-safe, single consistent file) plus an accompanying `cinma-settings.json` sidecar (excluding Keychain secrets).
- Imports verify `PRAGMA integrity_check` and `user_version <= SCHEMA_VERSION` on the candidate file before replacing `cinma.sqlite3`.

## Update (2026-11-17, review round 2)
- **Correction:** `export_backup` writes only the SQLite snapshot. App settings live in SQLite, so they are included; there is no separate `cinma-settings.json`. Secrets stay in Keychain.
- **Restore** now stages, fsyncs and validates the copy, then swaps by rename with rollback. The previous database is kept as `cinma.sqlite3.pre-restore`. Restore is refused while downloads run.
