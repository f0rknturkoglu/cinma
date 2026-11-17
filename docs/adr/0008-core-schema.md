# ADR 0008 — Core schema (migration 0002)

**Status:** proposed — owner review (data formats are ask-first). Easy to change until real data accumulates: migrations are append-only, so a change means a new migration.

## Conventions
- Every user-data table has `profile_id INTEGER NOT NULL REFERENCES profile(id) ON DELETE CASCADE`, and every repository query is scoped by it. A test checks that profile B can't read or delete profile A's rows.
- All tables are `STRICT`. Timestamps are unix seconds (`INTEGER`).
- Payloads from outside are stored as JSON text: addon manifests and download sources. They are parsed in Rust and not normalised.
- One connection per process, WAL, `foreign_keys=ON`, `busy_timeout` 5 s.

## Tables added
| Table | Scope | Purpose |
|---|---|---|
| `profile` + `avatar`, `pin_hash` | — | profile picker (Phase 3), PIN (Phase 7) |
| `addon` | global | installed addons: manifest id, transport URL, manifest JSON, user order |
| `profile_addon` | per profile | addons enabled per profile (§5.10) |
| `profile_setting` | per profile | language prefs, keep-watched, … |
| `download` | per profile | unified queue: kind, source JSON, state, priority, `blob_id`, byte counts, error |

Global settings stay in `app_setting`:
- `feature.<name>` = `0` / `1`;
- `keybinding.<command>` = combo, or `""` for unbound;
- `profile.active`.

## Upgrade path
Migration 0002 moves the Phase 1 addon list (`app_setting['addons.v1']` JSON) into `addon` and enables those addons for profile 1. It uses SQLite's built-in JSON functions; system SQLite here is 3.54. A test runs it on a v1 database with data.

## Deliberately not yet
Library, watch progress, meta cache, ID map and the sync outbox are left out. Each comes as its own migration in the phase that first uses it (3, 4 and 6), so the tables are designed against real code.
