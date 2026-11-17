# Cinma — handoff and roadmap

For the next agent. **Phases 0–7 are built and were reviewed** (two rounds of fixes, 2026-11-17). This file says what is left: what the owner must test, what the owner must decide, and the next phases (UX polish, integrations, hardening). The original Phase 3–7 task lists are in git history (`git show a385643:docs/ROADMAP.md`).

**Read first, in this order:**
1. `CLAUDE.md`: the standing brief. §6 holds the owner-approved targets (2026-11-17).
2. `PROGRESS.md`: what exists, what was measured, and what each review fixed.
3. `docs/RISKS.md`: open risks.
4. `docs/adr/`: architecture decision records (`0010–0016` owner-reviewed 2026-11-17).
5. This file.

**Working rules (owner and brief):**
- **Phases:** work one phase at a time. At the end of each phase, report in **Turkish** what was built, measurements against targets and risks, then wait for confirmation.
- **Git:** commit after each step. Never push unless asked.
- **Dependencies and decisions:**
  - A new dependency needs a one-paragraph justification.
  - Ask before data-format or engine changes.
  - Write a short ADR for any deviation from the brief.
- **Never add:** background processes, WebViews, telemetry, idle polling, or unbounded caches. `yt-dlp -J` is the only child process. The owner said a helper process *could* be discussed if it buys RAM or speed; ask first.
- **Code layout:** business logic lives in Rust; Swift is presentation only.
- **SSD:** the owner asked for minimum SSD wear, maximum speed and durability. Prefer moves and hard links over copies, and few fsyncs.

---

## 1. Current state in one screen

| Area | State |
|---|---|
| Browse | Home (hero, rails, Continue Watching), Discover, search, title page with seasons/episodes, Library, Calendar, addon manager, profiles with PINs, onboarding |
| Stream selection | Parallel addon fan-out; ranker (parse → filter → score → tier, with an explanation) |
| Player | libmpv via the OpenGL render API on a GL thread; subtitles (addons + OpenSubtitles, Turkish encodings), dual subs, thumbnails, next episode, stream switcher, recovery, A/B loop, stats, shaders |
| Torrent | librqbit. Streams are deleted on stop. Downloads are written once into `<folder>/.cinma-partial/<hash>` and moved into place (ADR 0016) |
| Downloads | One queue for torrent, segmented HTTP and yt-dlp. Play Now while downloading; resume after kill -9; files land in `~/Movies/Cinma` and `~/Music/Cinma` |
| Integrations (off by default) | Chromecast/DLNA (in-process mDNS/SSDP, a LAN server only while casting); Trakt/Simkl/AniList/MAL outbox with automatic sync; Letterboxd RSS/CSV; Stremio import |
| Data | SQLite schema v7, WAL, migrations; atomic backup/restore with rollback; secrets in Keychain |
| Quality | 120 Rust tests, clippy clean, Release build without warnings, en + tr strings complete |
| Measured | Launch 169 ms; idle 30 MB empty / 172 MB with artwork; 4K HDR ~17 % CPU, ~630 MB |

---

## 2. Owner test checklist (things the agent cannot do from its sandbox)

The agent's shell proxies outbound TCP, so it cannot reach BitTorrent peers, real LAN devices or OAuth services. Run these on a normal Mac and network, and report the results (numbers or screenshots) to the next agent.

**Build first:**
```bash
xcodebuild -project app/Cinma.xcodeproj -scheme Cinma -configuration Release -derivedDataPath build/xcode build
```

1. **First launch.** Move `~/Library/Application Support/Cinma` aside, open the app, and check that the welcome sheet appears over Home and that Skip and Continue both work.
2. **Real-swarm torrent speed** (targets: median first frame < 5 s, seek < 2 s). Use a Torrentio-style addon you trust and a popular title:
   ```bash
   tools/measure-playback.sh ADDON_MANIFEST_URL movie tt0111161 5
   ```
   If it misses, tune librqbit before anything else: peer limit, request window, the stream window (`PER_STREAM_BUF_DEFAULT` is 32 MB), trackers.
3. **Torrent download through the UI.** On a title page, choose Download on a torrent stream, then:
   - Play Now mid-download;
   - pause, resume, quit, reopen;
   - check that the file ends up in `~/Movies/Cinma` and that `~/Movies/Cinma/.cinma-partial` is gone afterwards.
4. **yt-dlp:**
   - install it from Settings;
   - download a normal video. It usually has separate video and audio tracks, which exercises the **untested mux path**;
   - download an audio-only item, which should land in `~/Music/Cinma`.
5. **Chromecast and DLNA.** Turn on Casting in Settings → Integrations, discover devices, cast a stream and a finished download. macOS should ask for local-network permission the first time; check that it does.
6. **Tracking.** Connect Trakt (device code; it needs your own Client ID and Secret from trakt.tv/oauth/applications). Watch something past 90 %, and check that it shows up on Trakt without pressing "Sync Outbox Now". Then try Simkl, AniList and MAL with tokens.
7. **Stremio import** with your account (the password is never stored).
8. **Media corpus.** Put real samples in `~/Movies/cinma-corpus` and play each one: Dolby Vision P5/P8/P7, AV1, ASS-heavy anime, multi-audio MKV, MKV with cues at the end, MP4 with moov at the end. Note wrong colours (DV P5 is expected to be wrong on the current render path, ADR 0004), missing subtitles and CPU.
9. **Menu bar only.** Close the window while a download runs, then check Activity Monitor memory (target ≤ 40 MB) and that the download finishes.
10. **Turkish UI.** Set the system language to Turkish, or launch with `--args -AppleLanguages "(tr)"`, and read the translations; the agent wrote them.

---

## 3. Owner decisions (2026-11-17 — settled)

| # | Decision | Outcome |
|---|---|---|
| 1 | **Relaxed targets** in `CLAUDE.md` §6 (`≤ 70 MB` empty, `≤ 200 MB` / `≤ 0.5 %` artwork idle, `< 20 %` / `≤ 700 MB` 4K HDR10) | **Approved.** Report if real media corpus samples exceed these bounds. |
| 2 | **Phase 3 & ADRs `0010–0016`** | **Approved** with scope notes: `0010, 0011, 0012, 0014, 0016` accepted; `0013` §3 menu bar accepted (`§1–2` superseded by `0016`); `0015` backup + `libmpv` lookup accepted (packaging rewritten in Phase 10); `0011` PiP updated per item 4. |
| 3 | **Torrent streaming storage** (disk vs RAM) | **Keep on disk** (delete on stop). 40–80 GB remux seeks need on-disk random access; measure real-world writes if needed later. |
| 4 | **PiP method** (`ADR 0011` §4) | **Floating mini-player `NSPanel`** reusing `VideoLayer` (zero-copy, keeps HDR & subtitles), scheduled in Phase 8 or Phase 9. Skip frame-copy system PiP. |
| 5 | **Render path D1** (`macvk` + custom `libmpv`/`libplacebo`, `ADR 0004`) | **Decide after owner corpus test** (DV/AV1/ASS). Stay on OpenGL until then; if DV P5 colours or CPU fail, build D1 in Phase 10 alongside bundled `libmpv`. |
| 6 | **Post-torrent memory (~113 MB)** | **Trial `mimalloc` or `jemalloc` in Phase 10** with before/after measurements + ADR; accept status quo if gain is negligible. |
| 7 | **Streaming cache quota & "keep watched"** | **Keep deleting streams on stop**; do not re-enable quota/keep-watched. Remove dormant code in Phase 10 (keep tables via migration). |
| 8 | **Distribution & custom `libmpv`/FFmpeg bundling** (`ADR 0015`) | **Bundle minimal `libmpv` + FFmpeg inside the app in Phase 10** (avoid `brew upgrade` ABI breakage); ad-hoc signing suffices for personal use, drop `disable-library-validation` once libraries share one signing identity (Developer ID / notarization only if distributed externally). |

---

## 4. How to build, test, measure

```bash
cargo test                       # all Rust tests (incl. kill -9 suites)
cargo clippy --all-targets       # must stay clean
tools/build-core.sh Release      # Rust lib + Swift bindings (Xcode does this too)
xcodebuild -project app/Cinma.xcodeproj -scheme Cinma -configuration Release \
  -destination 'platform=macOS,arch=arm64' -derivedDataPath build/xcode build
tools/measure.sh                 # launch + idle RSS/footprint/CPU → tools/measurements.csv
tools/measure-playback.sh "ADDON_URL" movie IMDB_ID 5   # TTFF + seek, real app
```

- **Prerequisite:** `brew install mpv`. libmpv is loaded at runtime from `/opt/homebrew/lib/libmpv.2.dylib`; override with `CINMA_LIBMPV`.
- **Deployment target** lives in two places: `app/Config/Cinma.xcconfig` and `tools/build-core.sh`.

**Dev environment variables** (see `app/Cinma/DevAutomation.swift`; all are inert unless set):

| Variable | Effect |
|---|---|
| `CINMA_MEASURE=1` | print launch, TTFF and seek metrics to stderr |
| `CINMA_DATA_DIR` | use this data dir instead of the default |
| `CINMA_DEV_ADDON` | install this addon at start-up |
| `CINMA_AUTOPLAY=movie:ID` / `file:/path` | start playing automatically |
| `CINMA_AUTOSEEK=0.5,0.85` | seek to these fractions of the duration |
| `CINMA_AUTOQUIT=1` / `close` | quit, or just close the player, when done |
| `CINMA_AUTOCLOSE_AFTER=secs` | close the player after this many seconds |
| `CINMA_DEV_OPEN_URL` | handle this URL as if macOS had opened it |
| `CINMA_DEV_PEERS=127.0.0.1:6999` | extra torrent peers (loopback seeder) |
| `CINMA_MPV_LOG=v` | verbose mpv log |

**Offline torrent loop (loopback seeder + fixture addon).** Use this whenever real swarms aren't reachable:

```bash
cargo build --release -p cinma-torrent --example local_seed
target/release/examples/local_seed /path/to/video.mkv 6999 6000000   # prints INFO_HASH, upload 6 MB/s
mkdir -p /tmp/fixture/stream/movie
echo '{"id":"local.fixture","version":"0.0.1","name":"Local Fixture","resources":["stream"],"types":["movie"],"idPrefixes":["tt"],"catalogs":[]}' > /tmp/fixture/manifest.json
echo '{"streams":[{"name":"Local","infoHash":"INFO_HASH","fileIdx":0}]}' > /tmp/fixture/stream/movie/tt0000001.json
(cd /tmp/fixture && python3 -m http.server 7700 --bind 127.0.0.1)
CINMA_DEV_PEERS=127.0.0.1:6999 tools/measure-playback.sh http://127.0.0.1:7700/manifest.json movie tt0000001 5
```

Test clips (generated HDR10 4K HEVC and 1080p H.264) are in `~/Movies/cinma-corpus`. `tools/media-corpus.md` lists the real samples still needed: Dolby Vision P5/P8/P7, AV1, ASS-heavy, multi-audio, MKV with cues at the end, MP4 with moov at the end.

---

## 5. Pitfalls already learned (don't re-discover these)

**Agent sandbox**
- Outbound TCP from the agent's shell is proxied: closed ports look open and BitTorrent peers never connect. Test torrents over loopback (section 4).
- Real-swarm numbers must be measured by the owner, or outside the sandbox. Don't try to bypass the sandbox.

**mpv embedding**
- `vo=gpu-next` + `macvk` ignores `wid` (whether an `NSView*` or a `CAMetalLayer*`) and opens its own window. Only the OpenGL render API embeds. ADR 0004 lists the ways out.

**`CAOpenGLLayer`**
- Colorspace and `wantsExtendedDynamicRangeContent` must be set **before** the GL surface exists; changing them later is ignored.
- Swapping layers mid-playback kills mpv's vo. That is why the layer is always PQ/EDR and mpv always outputs `target-trc=pq`.
- `loadfile` must wait until the render context exists ("render-ready gating" in `PlayerViewController.start()`), otherwise mpv fails with "No render context set".
- `copyCGLContext` / `copyCGLPixelFormat` must return retained objects, and the cached ones are released in `deinit`.
- Before `render_init`: drain `glGetError` and bind FBO 0.

**Threading and teardown**
- Never call blocking mpv APIs on the main thread. Everything callable from the UI uses `mpv_command_async`.
- Teardown order: `render_free` (GL context current) → `quit` → `mpv_terminate_destroy` on the event thread.
- The host view must outlive mpv: close waits for the `Shutdown` event, with a 3 s fallback.

**Torrents**
- librqbit has no public "piece verified" hook (ADR 0009).
- Set DHT persistence explicitly; the default writes to `~/Library/Caches/com.rqbit.dht`.
- `Session::stop()` plus dropping the session brings idle CPU and sockets back to ~0.

**Memory**
- After a torrent playback, footprint stays at ~113 MB even though the live heap is ~13 MB (malloc fragmentation). `malloc_zone_pressure_relief` doesn't help.

**Strings**
- `xcodebuild` does not update `Localizable.xcstrings`. Build Debug, then sync the catalog from the compiler's extraction and fill in `tr` for the new keys (don't guess keys with a regex):
  `xcrun xcstringstool sync app/Cinma/Localizable.xcstrings --stringsdata …` (all `*.stringsdata` under `build/xcode/Build/Intermediates.noindex/Cinma.build/Debug`). Multi-argument keys get positional `%1$@` in `en`.
- Check by launching in Turkish: `open -n build/xcode/Build/Products/Release/Cinma.app --args -AppleLanguages "(tr)"`.

**Xcode project**
- `project.pbxproj` is hand-written. Files dropped into `app/Cinma/` are picked up automatically (synchronized folder).
- `Info.plist` is generated and merged with `app/Config/Info.plist`, which holds the URL schemes.

**Tests**
- Child-process tests: libtest prints `test name ... ` before the child's first line, so print a leading `\n` first.
- The parent must not create the DB itself; wait for the child to report "ready".

**Mouse and keyboard automation**
- Paste through the menu is blocked by the tool; use typing instead.

**Learned in the Phase 3–7 review**
- **AppKit:** never `presentAsSheet` from `viewDidLoad`; the view has no window yet. It becomes a stray off-screen window and the content swap never finishes. Present from `viewDidAppear`.
- **librqbit persistence** (`fastresume: true`) restores every torrent it ever saw, unpaused, on the next session. Streams are tracked in `streams.json` and deleted on stop; downloads are detached with `forget()` on pause or finish.
- **Downloads go to `~/Movies/Cinma`** by default. Tests must point the disk-policy folders at a temp dir, or they write into the owner's real Movies folder.
- **Never delete by a stored folder path.** Only delete exact paths you built: the blob, `staging_dir(folder, hash)`, `download.file_path`. In the UI, finished files go to the Trash only after the user asks.
- **Diffable data sources** hold ids only. Call `reloadItems` for rows whose content changed, or progress never redraws.
- **Pill buttons** (`CinemaTheme.makePillButton`) carry their own padding (`PillButton`). Don't add fixed widths.
- **Run `measure.sh` and UI checks** with `open -n --env CINMA_DATA_DIR=…`. Use a fresh data dir to exercise first launch: `CINMA_MEASURE` skips onboarding, so it hides first-launch bugs.
- **Timing tests:** warm up `dlopen`'d libraries before timing anything; under parallel test load they take longer than 50 ms.

**Shell**
- In zsh, `echo =====` is an "equals expansion" and fails. Quote separators.
- When giving the owner commands, never use `<placeholders>`; an apostrophe inside one hangs zsh. Use plain `ADDON_URL`-style words.

---

## 6. Owner decisions so far

| Decision | Where |
|---|---|
| Launch measured against the AppKit floor; memory measured as `phys_footprint` | ADR 0003 |
| Defaults: macOS 15 minimum, arm64 only, ad-hoc signing, unsandboxed, system SQLite | ADR 0002 |
| Render path: stay on the OpenGL render API for now (no Dolby Vision P5) | ADR 0004 |
| Post-torrent memory (~113 MB): later | PROGRESS |
| Schema conventions; addon identity = manifest id | ADR 0008 |
| Torrents stay on librqbit storage, outside BlobStore | ADR 0009 |
| `stremio://` links ask before installing | done |
| RAM/CPU targets may be loosened "a little" (numbers pending, §3 item 1) | CLAUDE.md §6 |
| Minimum SSD wear, maximum speed and durability | ADR 0016 |
| A helper process is negotiable if it buys RAM or speed (ask first) | — |

---

## 7. Phase 8 — UI/UX polish (next)

Goal: the product feels finished and matches §5.15 ("dark cinema + Liquid Glass"). Do a design pass first, then fix by screen. Use the `better-interface` / `better-ui` / `better-writing` skills if available. Check every change in English and Turkish, with Reduce Motion and Reduce Transparency on.

**Known problems (seen during the review):**
1. **Raw errors in the UI.** The stream picker shows `Cinma.CinmaError.Invalid(message: …)` when no addon serves streams. Route every user-visible error through `FriendlyError` with an action ("Add a stream addon", "Try another stream").
2. **Downloads table:**
   - Row height and the cell pill buttons are cramped; "Play Now" is cut off in the row.
   - Progress comes from SQLite (every 10 s). Consider live progress from the engine, in memory, so the bar moves smoothly without extra disk writes.
3. **Home hero:** the gradient scrim covers only a left box, and the edges are visible on wide windows. The hero rotates every 10 s even when the window is in the background; pause it when occluded (`NSWindow.occlusionState`).
4. **Onboarding:** check spacing, the default language (it shows Türkçe on an English system), and validation for the addon URL and TMDB key.
5. **Image memory:** Home with artwork is 172 MB. Decode backdrops at display size, and lower the `NSCache` cost limit when the window is not key.
6. **Settings:** it is long and flat. Split it into tabs (General, Playback, Subtitles, Downloads, Integrations, Backup). Show the yt-dlp status and folders clearly.
7. **Empty, loading and partial states** on every screen. Streams arrive per addon; show skeletons and "n of m addons answered".
8. **Keyboard and accessibility:** full keyboard navigation for rails and grids, focus rings in the accent colour, VoiceOver labels on poster cards (titles are withheld today), Escape and Backspace behaviour.
9. **Player HUD:** check track menus, the next-episode card and the thumbnail scrubber against §5.15. Auto-hide timing; no flicker over HDR.
10. **App icon, About window and credits** (licences: mpv/FFmpeg LGPL, librqbit, etc.).

**Exit:** a screenshot set of every §5.15 screen in en and tr, no raw errors, keyboard-only navigation works, and the §6 targets still hold.

---

## 8. Phase 9 — Integrations and add-ons to write

Each one is a new `Feature` (off by default, never constructed when off) or a provider behind an existing trait. Before each: write a five-line ADR, justify any dependency, and test against a local mock. Leave real-service checks to the owner.

| Priority | Integration | Notes |
|---|---|---|
| 1 | **Skip intro/outro: AniSkip (anime) + IntroDB** | Uses the ID map; shows the Skip button the player already has. Cache in SQLite with a TTL |
| 2 | **Debrid resolvers** (Real-Debrid, AllDebrid, Premiumize, TorBox) | Torrentio's debrid mode already works as plain URLs; a native resolver adds cache checks and the user's own key (Keychain). Rank "cached" streams up |
| 3 | **OAuth sign-in for Simkl, AniList and MAL** (today they paste a token) and **two-way Trakt** (watchlist, ratings, history pull) | Uses the existing outbox; device code or PKCE in the system browser, never a WebView |
| 4 | **OpenSubtitles account UI** (login, quota display) and subtitle search by hash in the player | The client exists (`cinma-subtitles`) |
| 5 | **Calendar notifications** for new episodes in the library (`UserNotifications`, scheduled locally) | No background process: schedule when the app runs |
| 6 | **Fanart.tv logos/backdrops** and TMDB watch providers on the title page | Needs the user's key |
| 7 | **Kitsu metadata** for anime and the AniList/MAL mapping refresh (the dataset is user-refreshable) | §5.13 |
| 8 | **Archive playback** (store-mode RAR/ZIP inside torrents) | A `ByteSource` wrapping a `ByteSource` (slot exists) |
| 9 | **Live TV / IPTV** (M3U, Xtream, XMLTV EPG) | New screen; plays through mpv |
| 10 | **Discord Rich Presence** | In-process IPC over Discord's local socket; off by default |
| 11 | **VPN interface binding / kill switch** for torrent traffic | librqbit `bind_device_name` |
| 12 | **App Intents / Shortcuts** (play the next episode, open a title) and Spotlight for the library | Optional |
| 13 | **PiP mini-player (`NSPanel`)** | Reuses `VideoLayer` (zero-copy, keeps HDR & subtitles; may also land in Phase 8) |

Out of scope: Usenet.

---

## 9. Phase 10 — Hardening and release

- **Real-swarm tuning** from checklist item 2; **post-torrent memory trial** (`mimalloc` vs `jemalloc` with before/after measurements + ADR; accept current if negligible); idle CPU after torrent playback (~0.13 %, tokio/hyper timers suspected).
- **Minimal bundled `libmpv` + FFmpeg** in `Contents/Frameworks` (`ADR 0015`) so Homebrew upgrades cannot break ABI; pair with render path D1 (`macvk` + `libplacebo`) if owner corpus tests show DV P5 colour or CPU issues.
- **Signing & code hygiene:** drop `disable-library-validation` as soon as bundled libraries share one signing identity (ad-hoc for personal use; Developer ID + notarization + hardened runtime only if distributed externally); **remove dormant disk-quota & `keep_watched` code** (leave schema tables intact via migration).
- **Crash safety sweep:** kill -9 tests for yt-dlp mux, casting and restore, like the existing blob/queue/HTTP ones.
- **Competitor comparison** (§2 of the brief) with `tools/measure.sh` against Stremio v5, IINA and Flux on the same Mac, written into PROGRESS.
- **Optional local-only diagnostics:** a "Copy diagnostics" button (versions, recent errors). Never sent anywhere.
