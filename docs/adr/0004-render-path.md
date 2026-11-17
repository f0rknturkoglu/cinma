# ADR 0004 — Render path

**Status:** proposed — **needs owner decision**. Phase 1 ships option B as a stopgap.

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

## Recommendation
Keep B through Phase 3, since everything else works with it. In Phase 4, once DV P5/P8 samples are available, either spend about a day hardening C, or go straight to D as part of the custom libmpv build. D is the long-term answer; C is the quick one.
