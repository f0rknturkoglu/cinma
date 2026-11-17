# ADR 0003 — Launch/memory targets vs. the AppKit floor

**Status:** proposed — needs owner decision

## Finding

A trivial AppKit app (one `NSWindow`, nothing else, `swiftc -O`) on this machine (M4 Pro, macOS 27.3, direct exec, warm) measures:

| | first frame | RSS | phys_footprint |
|---|---|---|---|
| Bare AppKit floor | ~152 ms | 77.5 MB | 17 MB |
| Cinma Phase 0 | ~150 ms (median of 7) | 82 MB | 20 MB |

~70 ms of the launch is `NSApplication.shared` alone, ~30 ms is AppKit's `finishLaunching`; both are outside our control. Our own code (menu, window, home view, first frame) costs ~45 ms, mostly first-use framework costs (first `NSWindow` ≈ 20 ms).

## Consequences

1. **Phase 0 exit "< 150 ms"** sits at the OS floor on this machine, so it's met only by a hair and is noise-sensitive. The §6 target (**< 250 ms**) is met comfortably.
2. **RSS cannot reach ≤ 70 MB**: macOS RSS counts shared dyld-cache pages, and bare AppKit is already at 77 MB. RSS mostly measures the OS.

## Proposal

- Track **phys_footprint** (what Activity Monitor shows) as the memory metric; restate targets as idle window ≤ 40 MB footprint (stretch ≤ 30), menu-bar-only ≤ 25 MB. Keep RSS in the CSV for reference.
- Track launch as **"overhead over the AppKit floor"** (currently ~0–10 ms) plus the absolute < 250 ms.
