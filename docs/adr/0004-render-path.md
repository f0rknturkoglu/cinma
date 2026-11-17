# ADR 0004 — Render path: libmpv render API on `CAOpenGLLayer` (EDR)

**Status:** accepted (Phase 1). Provisional on Dolby Vision (no P5/P8 samples yet).

## What was tried

Homebrew libmpv 0.42 (libplacebo 7.387, MoltenVK 1.4.2), M4 Pro, built-in XDR display.

**A — `vo=gpu-next`, `gpu-context=macvk`, `wid=<pointer>`: does not embed.**
mpv ignores `wid` on macOS and opens its own borderless window ("<file> - mpv"),
whether `wid` is an `NSView*` or a `CAMetalLayer*`. The first spike looked like it
worked only because the screenshot captured mpv's window rather than ours. libmpv 0.42 also
ships no Vulkan/Metal render API (only `render_gl.h`, `render_d3d11.h`), so there is
no supported way to put gpu-next output into our view.

**B — `vo=libmpv` + OpenGL render API into our `CAOpenGLLayer`: works.**
The video sits in our window under our own controls.

## Measurements (option B, in the app, Release)

| Clip | hwdec | CPU (process, one core) | EDR | Notes |
|---|---|---|---|---|
| 4K HEVC Main10 HDR10 (PQ, 24 fps, generated) | videotoolbox | ~19 % | engaged: layer in `itur_2100_PQ`, 16-bit float FB, screen headroom 7.3 (potential 16) | 0 dropped frames in the spike |
| 1080p H.264 SDR (30 fps, generated) | videotoolbox | ~21 % | n/a | |

The spike version of A measured ~14 % (in its own window), and the B spike ~15 %.

## Decision

**B.** It is the only path that renders into our UI with the stock libmpv. It is IINA's proven approach.

- `VideoLayer` (Swift) owns one CGL context and a 64-bit float pixel format, with `wantsExtendedDynamicRangeContent`.
- mpv's render context is created inside the first `draw` with that context current.
- On `video-params/gamma` = `pq` or `hlg`, the layer switches to `itur_2100_PQ` and mpv to `target-trc=pq`, `target-prim=bt.2020`.
- Teardown order is: `render_free` (GL context current), then `quit`, then `mpv_terminate_destroy` on the event thread.

## Consequences / follow-ups

- OpenGL is deprecated. If Apple removes it, the options are a custom libmpv with a Metal/Vulkan render API, or vo=gpu-next with a patched `wid` for macvk. Revisit this together with the custom libmpv build (ADR 0005).
- Playback CPU (~20 %) is above the §6 target (< 10 % for 4K HDR10). Phase 4 should look at:
  - drawing on a dedicated CA/GL thread (`isAsynchronous`) instead of the main thread;
  - avoiding the float FB for SDR;
  - `video-sync` and the per-frame callback cost.
- Playback footprint is 470–630 MB (GPU surfaces, demuxer cache). Phase 4 should tune it.
