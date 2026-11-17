# ADR 0004 — Render path: gpu-next + macvk embedded via `wid`

**Status:** accepted, provisional (Dolby Vision untested — no P5/P8 samples yet)

## Options measured (Phase 1 spikes, Homebrew libmpv 0.42 / libplacebo 7.387 / MoltenVK 1.4.2)

Clip: generated 4K HEVC Main10 HDR10 (PQ, BT.2020, 1000-nit mastering), 24 fps, `hwdec=videotoolbox`.
Machine: M4 Pro, built-in Liquid Retina XDR. CPU = process CPU time over 5 s of steady playback.

| | A: `vo=gpu-next`, `gpu-context=macvk`, `wid=<NSView*>` | B: `vo=libmpv` render API on `CAOpenGLLayer` (EDR) |
|---|---|---|
| Embeds in our view | yes — mpv adds its own Metal layer under our `NSView` | yes — we own the layer |
| hwdec | videotoolbox | videotoolbox |
| CPU (one core) | ~14 % | ~15 % |
| Dropped / delayed frames | 0 / 0 | 0 / 0 |
| HDR | automatic: PQ swapchain (`HDR10_ST2084`), `CAEDRMetadata` 1000 nits, "HDR: active" | manual: we must set layer colorspace + `target-trc/prim/peak` per file (IINA-style) |
| Issues | first init spends ~260 ms enumerating Vulkan extensions (once per player) | `INVALID_OPERATION` on texture creation; OpenGL is deprecated |
| Our code | ~0 (pass a pointer) | ~150 lines of GL + per-file HDR logic |

## Decision

**A (gpu-next + macvk via `wid`)**. Same CPU, HDR/EDR handled by libplacebo with metadata passthrough, far less code, no deprecated API. Everything mpv-related can live in Rust; Swift only provides an `NSView`.

## Consequences

- All mpv calls (create/initialize/loadfile/terminate) happen off the main thread; mpv's macOS code dispatches to the main queue itself, so the main thread must never block on mpv.
- The host `NSView` must outlive the mpv handle.
- Both paths are above the §6 target (< 10 % for 4K HDR10). Re-measure in Phase 4 with real content and `video-sync`/`interpolation` settings; the synthetic clip is high-entropy.
- Dolby Vision P5/P8 must be checked once samples are available (`tools/media-corpus.md`); revisit this ADR if libplacebo/MoltenVK mishandles them.
