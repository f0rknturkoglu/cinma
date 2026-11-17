# Risks and open decisions

Updated 2026-11-17 after the Phase 3–7 review. The owner's test checklist and next phases are in `docs/ROADMAP.md`. Fixed items are recorded in `PROGRESS.md`; this file lists only what is still open.

## Not yet verified on real networks or devices
- **Torrent TTFF and seek on a real swarm.** The agent sandbox proxies outbound TCP, so only the loopback seeder was tested. On a normal network run: `tools/measure-playback.sh "<addon manifest url>" movie <imdb id> 5`.
- **Chromecast / DLNA.** The in-process mDNS and SSDP discovery and the CastV2/SOAP control are tested with synthetic packets and local mocks. A real device test is owner-side. macOS may ask for local network permission the first time you cast.
- **Trakt / Simkl / AniList / MAL / Stremio / OpenSubtitles / Debrid / SkipSegments / Fanart / Kitsu / IPTV / Discord IPC:** all Phase 6 & Phase 9 clients are tested against local mock servers or Unix sockets (`cargo test`); real-account credentials, live OAuth browser redirect round trips, live Discord desktop IPC, and VPN `utun*` kill-switch behavior are owner-side checks.

## Media and rendering
- **Dolby Vision P5 & corpus:** the OpenGL render path (ADR 0004) stays active until real samples (`DV P5/P8/P7`, `AV1`, `ASS-heavy`, `multi-audio` in `tools/media-corpus.md`) are tested by the owner. If DV P5 colours are wrong or CPU exceeds limits, D1 (`libmpv` + `macvk`/`libplacebo`) will be bundled in Phase 10.
- **Homebrew FFmpeg/libmpv dependency:** major version bumps via `brew upgrade` break dlopen thumbnails/muxing; Phase 10 will bundle a minimal `libmpv` + FFmpeg build inside `Contents/Frameworks` and drop `disable-library-validation` (ADR 0015).
- **AirPlay video:** not possible with libmpv; audio goes to AirPlay speakers through CoreAudio.

## Resources (targets owner-approved 2026-11-17; see CLAUDE.md §6)
- **4K HDR10 playback:** ~17 % CPU and ~630 MB, inside approved targets (< 20 %, ≤ 700 MB). Real corpus samples still to be checked.
- **Home with artwork:** 172 MB idle (≤ 200 MB target). Decoded backdrops and the 64 MB image cache are the bulk.
- **Memory after torrent playback** (~113 MB, malloc fragmentation): `mimalloc` or `jemalloc` trial with before/after measurements scheduled for Phase 10 (keep status quo if gain is negligible).
- **SSD:** torrent streaming writes sequential pieces to disk and deletes on stop (approved: 40–80 GB remux files cannot stay in RAM; measure actual writes in real use if needed). Downloads are written once (ADR 0016), with checkpoints every 10 s; dormant quota / keep-watched code will be removed in Phase 10.
