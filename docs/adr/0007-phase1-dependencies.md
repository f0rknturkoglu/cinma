# ADR 0007 — Phase 1 dependencies

Rust crates added in Phase 1:

- **librqbit 9.1** (Apache-2.0, actively maintained) — the torrent engine the brief mandates. Built with `default-features = false`, `rust-tls`, so there is no HTTP API or web UI. Note: it pulls `aws-lc-sys` (C code; builds with the stock toolchain) for SHA-1/TLS.
- **tokio 1.54** (MIT) — required by librqbit and reqwest. One runtime with 2 workers, created on first async use.
- **tokio-util 0.7** (MIT) — `CancellationToken`, used for cancellable blocking reads (mpv `cancel_fn`).
- **reqwest 0.13** (MIT/Apache-2.0), `rustls` feature — HTTP for the addon protocol. It's the same major version librqbit uses, so there's no duplicate.
- **serde / serde_json / url** (MIT/Apache-2.0) — addon JSON and URL handling. Already in the tree through librqbit.
- **libloading 0.9** (ISC) — dlopen of libmpv and of the OpenGL framework (lazy loading, ADR 0005).
- **anyhow 1** (MIT/Apache-2.0) — only to convert librqbit's error type; it doesn't appear in our public APIs.

Cost:
- 273 unique crates in the `cinma-ffi` dependency tree.
- The app binary grew from 0.7 MB to 13 MB.
- Launch first frame stays at 158 ms (Phase 0: 150–160).
- Idle footprint went from 20 MB to 25 MB.
