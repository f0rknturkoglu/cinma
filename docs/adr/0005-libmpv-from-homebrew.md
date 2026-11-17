# ADR 0005 — libmpv from Homebrew for now, loaded with dlopen

**Status:** accepted (Phase 1)

- The brief asks for a custom-built libmpv (minimal FFmpeg, libplacebo, VideoToolbox). That is a sizeable build project and not needed to get video on screen, so Phase 1 uses Homebrew's `mpv` (0.42, libplacebo 7.387, MoltenVK 1.4.2): `brew install mpv`.
- libmpv is **never linked**. `cinma-player` dlopens it on first playback (`CINMA_LIBMPV` env override, else `/opt/homebrew/lib/libmpv.2.dylib`), so launch time and idle memory don't pay for FFmpeg & co.
- Revisit before Phase 7 (packaging): build a minimal libmpv and ship it inside the app bundle; the dlopen search path gains `Contents/Frameworks` first.
