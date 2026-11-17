# PROGRESS

## Current phase: 0 — Skeleton (built; awaiting owner review)

### Done
- Rust workspace: `cinma-core`, `cinma-db`, `cinma-ffi` (+ `tools/uniffi-bindgen`). 7 tests green.
- SQLite: WAL, `STRICT` tables, transactional migrations over `user_version`, refuses newer schemas. Migration 0001 is a scaffold (`profile`, `app_setting`) — the real schema is Phase 2.
- UniFFI wiring: `CinmaCore.open(dataDir:)`, `schemaVersion()`, `coreVersion()`; opened off the main thread after the first frame.
- AppKit app: manual `main.swift`, no storyboard, no SwiftUI; dark #0B0B0D window; programmatic main menu.
- String Catalog (`Localizable.xcstrings`) with English + Turkish; verified `tr.lproj` in bundle and Turkish UI.
- `tools/build-core.sh`, `tools/measure.sh`, ADRs 0001–0003.

### Measured (M4 Pro, macOS 27.3, Release, direct exec, warm, median of 7)
| Metric | Value | Target |
|---|---|---|
| First frame | 150.5 ms (other runs: 158, ~160) | < 150 ms (Phase 0): **not met as written** — at the OS floor / < 250 ms (§6): met |
| Core ready (DB open) | 162 ms | — |
| RSS | 82.3 MB | ≤ 70 MB — below the AppKit floor, see ADR 0003 |
| phys_footprint | 20.0 MB | — |
| Idle CPU | 0.0 % | ~0 % |
| Bundle size | 0.7 MB | — |

Bare-AppKit floor on the same machine: ~152 ms, 77.5 MB RSS, 17 MB footprint.
"First frame" = first main-queue turn after `makeKeyAndOrderFront` (approximate; same method for the floor app). M4 Pro, not the M1 baseline. Launch via `open` (LaunchServices) is ~15–30 ms slower than direct exec.

### Open questions for the owner
1. ADR 0003: accept launch as "overhead over AppKit floor" (Phase 0 < 150 ms exit not met as written)?
2. ADR 0003: switch memory targets to phys_footprint?
3. ADR 0002 defaults (macOS 15 minimum, unsandboxed, ad-hoc signing, system SQLite).

### Next: Phase 1 — vertical slice
Addon by URL → streams → librqbit → BlobStore → `mpv_stream_cb` → video on screen; render-path ADR.

### Risks
- Launch target is at the OS floor; regressions show up as noise first. Watch the "overhead over floor".
- libmpv custom build (minimal FFmpeg + libplacebo + MoltenVK?) is the biggest unknown for Phase 1.
