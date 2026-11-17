# Risks and open decisions

Updated 2026-11-17 after the Phase 3–7 review. The owner's test checklist and next phases are in `docs/ROADMAP.md`. Fixed items are recorded in `PROGRESS.md`; this file lists only what is still open.

## Not yet verified on real networks or devices
- **Torrent TTFF and seek on a real swarm.** The agent sandbox proxies outbound TCP, so only the loopback seeder was tested. On a normal network run: `tools/measure-playback.sh "<addon manifest url>" movie <imdb id> 5`.
- **Chromecast / DLNA.** The in-process mDNS and SSDP discovery and the CastV2/SOAP control are tested with synthetic packets and local mocks. A real device test is owner-side. macOS may ask for local network permission the first time you cast.
- **Trakt / Simkl / AniList / MAL / Stremio import.** Tested only against local mock servers.

## Media and rendering
- **Dolby Vision P5:** the OpenGL render path (ADR 0004) has no libplacebo, so DV P5 colours are not handled. Solution D1 in ADR 0004 (custom libmpv + macvk) applies if real samples look wrong.
- **Media corpus:** DV P5/P8/P7, AV1, ASS-heavy and multi-audio samples are missing (`tools/media-corpus.md`).
- **Homebrew FFmpeg/libmpv major versions** are checked at runtime. A different major version disables thumbnails and muxing until a matching build is bundled (ADR 0011/0015). The app keeps `disable-library-validation` until everything is signed with one identity.
- **PiP:** needs the owner to choose between a floating mini-player panel and a frame-copy PiP (ADR 0011).
- **AirPlay video:** not possible with libmpv; audio goes to AirPlay speakers through CoreAudio.

## Resources (relaxed targets proposed 2026-11-17, awaiting owner confirmation; see CLAUDE.md §6)
- **4K HDR10 playback:** ~17 % CPU and ~630 MB, inside the relaxed targets (< 20 %, ≤ 700 MB).
- **Home with artwork:** 172 MB idle (≤ 200 MB target). Decoded backdrops and the 64 MB image cache are the bulk.
- **Memory after torrent playback** (~113 MB, malloc fragmentation): deferred by the owner.
- **SSD:** streaming writes torrent pieces to disk (ADR 0009) and deletes them when playback stops. Streaming from RAM only would be an engine change and is the owner's call. Downloads are written once (ADR 0016), with checkpoints every 10 s.

## Pending owner decisions
- ADRs 0011–0016 are "proposed".
- Phase 3 and ADR 0010 are marked "accepted (owner)" by the previous agent. The owner has not confirmed this.
