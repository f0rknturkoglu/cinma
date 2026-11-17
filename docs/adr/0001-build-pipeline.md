# ADR 0001 — Rust ↔ Swift build pipeline

**Status:** accepted (Phase 0)

- **Xcode project is hand-written** (`objectVersion 77`, file-system-synchronized `app/Cinma/` folder). Adding a Swift file = drop it in the folder; no project edits. No XcodeGen/Tuist — one less tool.
- **Rust core links as a static library** (`libcinma_ffi.a`, arm64 only). The `cdylib` is built only so `uniffi-bindgen` can read metadata in library mode.
- **UniFFI bindings are generated every build** by `tools/build-core.sh` into `app/Generated/` (gitignored); files are copied only when changed so Swift doesn't recompile needlessly. The bindgen binary lives in the workspace (`tools/uniffi-bindgen`) so it always matches the runtime `uniffi` version.
- The generated Swift is compiled **inline into the app target** under Swift 6 strict concurrency (compiles cleanly; no separate bindings target needed).
- Link flags come from `cargo rustc -- --print native-static-libs` (`-lsqlite3`, rest is libSystem). Re-check when adding crates with native deps.
- Verified: build from a clean tree (`app/Generated` deleted) succeeds in one `xcodebuild` invocation.
