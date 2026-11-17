# ADR 0004 — Render path

**Status:** accepted (owner, 2026-11-17): **stay on B (OpenGL render API) for now**; revisit with the solution paths below when Dolby Vision or OpenGL removal forces it. Owner also accepted ~19 % CPU for 4K HDR10 as good enough for now.

All findings use Homebrew libmpv 0.42 (libplacebo 7.387, MoltenVK 1.4.2) on an M4 Pro with the built-in XDR display.

## A — `vo=gpu-next`, `gpu-context=macvk`, `wid=<pointer>`: does not embed
- mpv ignores `wid` on macOS and opens its own borderless window ("<file> - mpv"). This happens whether `wid` is an `NSView*` or a `CAMetalLayer*`.
- An early spike looked like it worked only because the screenshot captured mpv's window.
- libmpv 0.42 ships no Vulkan/Metal render API, only `render_gl.h` and `render_d3d11.h`.

## B — `vo=libmpv` OpenGL render API into our `CAOpenGLLayer`: works, current choice
- Video renders inside our view, under our own controls. This was verified through the real UI.
- **Uses mpv's legacy `gpu` renderer, not libplacebo/gpu-next.** On OpenGL 4.1 there are no compute shaders, so HDR peak detection is off.
- **Dolby Vision P5 needs libplacebo, so B can't meet the brief's DV requirement.** HDR10/HLG work.
- HDR: the layer is created as a fixed PQ/BT.2020 EDR surface with a 16-bit float framebuffer. mpv always outputs `target-trc=pq`, `target-prim=bt.2020`, and SDR is mapped into PQ.
  - CAOpenGLLayer ignores colorspace/EDR changes after its surface exists.
  - Swapping layers mid-playback kills mpv's video output.
  - Measured: EDR headroom goes to 7–16 (potential 16) while playing. Side effect: EDR is engaged for SDR too.
- CPU on the generated clips: ~19 % (4K HEVC HDR10), ~21 % (1080p H.264). The target is < 10 %.
- OpenGL is deprecated.

## C — macvk window, view re-parented into ours: works partially, unsupported
- mpv's window lives in our process, so its content view (the Metal layer) can be moved into our view hierarchy and the window hidden.
- Spike result: gpu-next + libplacebo, "HDR: active" with CAEDRMetadata, our overlay drawn on top, one on-screen window.
- **But** the picture was cropped and scaled wrong: mpv keeps sizing to its own window. Resizing the hidden window to track ours might fix it; that is untested.
- Relies on mpv internals (`MacCommon` window/view), so any mpv update can break it.

## D — custom libmpv build that renders gpu-next into a host surface: robust, costly
- Either patch macvk to honour `wid` (adopt a given `CAMetalLayer`), or add a Vulkan render API.
- This fits naturally with the custom minimal libmpv build the brief already asks for (ADR 0005).
- It is a C/meson build project, plus a small patch to maintain.

## Solution paths for B's limits (ordered by cost)

The effort figures are rough guesses, and the mpv source locations and upstream status are from memory. Verify them when picking a path.

What B lacks: Dolby Vision P5 (needs libplacebo), HDR peak detection (needs compute shaders), and a future-proof API (OpenGL is deprecated).

1. **Harden C (≈1 day, quick, fragile).** Reparent mpv's macvk view into our view. Keep mpv's hidden window the same size as our view (`setFrame` on resize/fullscreen/backing-scale change) so mpv renders at the right size. Remove the view again before `quit`. Gate it behind a setting with B as fallback. Risk: depends on mpv's private `MacCommon` structure, so pin the mpv version.
2. **D1 — patch macvk to honour `wid` (≈2–4 days, inside the custom libmpv build from ADR 0005).** In `video/out/mac/*` + `vulkan/context_mac`, when `wid` is set, take the given `CAMetalLayer*` instead of creating a window (exact files: to be checked in the mpv tree) and report its size from the layer. The patch is small (mostly Swift in mpv's tree) and is a good candidate to upstream, which would remove the maintenance burden.
3. **D2 — add a Vulkan render API to libmpv (≈1–2 weeks).** An `MPV_RENDER_API_TYPE_VULKAN` backend for libmpv's render API; upstream interest is unverified. We'd own the `CAMetalLayer` + MoltenVK swapchain. Cleanest architecture, most work.
4. **Upstream first.** Watch or ask mpv for macvk `wid` support or a gpu-next render API. If it lands, option 2 or 3 becomes free.
5. **Not recommended:** our own libplacebo + FFmpeg pipeline without mpv (we'd re-implement subtitles, A/V sync, filters).

Triggers to act: the first Dolby Vision P5/P8 sample that plays with wrong colours, a macOS release that drops OpenGL, or the custom libmpv build starting (do D1 then).

## CPU (accepted for now)

The ~19–21 % in B can likely be reduced if it ever matters:
- render on a dedicated GL/CA thread (`isAsynchronous` or our own display link) instead of the main thread;
- `video-sync=display-resample` vs audio;
- check how often the update callback actually fires per vsync.
